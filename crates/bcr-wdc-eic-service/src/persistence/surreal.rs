use crate::{
    email,
    error::{Error, Result},
    service::EmailConfirmationRepository,
};
use anyhow::anyhow;
use async_trait::async_trait;
use bcr_common::core::NodeId;
use bcr_wdc_shared::{TStamp, now};
use bitcoin::secp256k1::schnorr::Signature;
use email_address::EmailAddress;
use surrealdb::{RecordId, Surreal, engine::any::Any};

type SurrealResult<T> = std::result::Result<T, Box<surrealdb::Error>>;

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct EmailConnectionConfig {
    pub connection: String,
    pub namespace: String,
    pub database: String,
    pub table_confirmations: String,
    pub table_registrations: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmailConfirmationDBEntry {
    pub node_id: NodeId,
    pub company_node_id: Option<NodeId>,
    pub email: EmailAddress,
    pub confirmation_code: String,
    pub wrong_entries: usize,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: TStamp,
}

impl From<EmailConfirmationDBEntry> for email::Confirmation {
    fn from(value: EmailConfirmationDBEntry) -> Self {
        Self {
            node_id: value.node_id,
            company_node_id: value.company_node_id,
            email: value.email,
            confirmation_code: value.confirmation_code,
            wrong_entries: value.wrong_entries,
            created_at: value.created_at,
        }
    }
}

impl From<email::Confirmation> for EmailConfirmationDBEntry {
    fn from(value: email::Confirmation) -> Self {
        Self {
            node_id: value.node_id,
            company_node_id: value.company_node_id,
            email: value.email,
            confirmation_code: value.confirmation_code,
            wrong_entries: value.wrong_entries,
            created_at: value.created_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmailRegistrationDBEntry {
    pub node_id: NodeId,
    pub company_node_id: Option<NodeId>,
    pub email: EmailAddress,
    pub mint_signature: Signature,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: TStamp,
}

impl From<EmailRegistrationDBEntry> for email::Registration {
    fn from(value: EmailRegistrationDBEntry) -> Self {
        Self {
            node_id: value.node_id,
            company_node_id: value.company_node_id,
            email: value.email,
            mint_signature: value.mint_signature,
            created_at: value.created_at,
        }
    }
}

const MIGRATIONS_TABLE: &str = "migrations";
const MIGRATION_ID: &str = "sqlx";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct MigrationDBEntry {
    #[serde(with = "time::serde::rfc3339")]
    migrated_at: TStamp,
}

#[derive(Debug, Clone)]
pub struct DBEmails {
    db: Surreal<Any>,
    table_confirmations: String,
    table_registrations: String,
}

impl DBEmails {
    pub async fn new(cfg: EmailConnectionConfig) -> SurrealResult<Self> {
        let db_connection = Surreal::<Any>::init();
        db_connection.connect(cfg.connection).await?;
        db_connection.use_ns(cfg.namespace).await?;
        db_connection.use_db(cfg.database).await?;
        Ok(Self {
            db: db_connection,
            table_confirmations: cfg.table_confirmations,
            table_registrations: cfg.table_registrations,
        })
    }

    /// Marks this database as migrated to PostgreSQL: from then on every write is refused
    pub async fn mark_migrated(&self) -> Result<()> {
        let entry = MigrationDBEntry { migrated_at: now() };
        let _res: Option<MigrationDBEntry> = self
            .db
            .upsert((MIGRATIONS_TABLE, MIGRATION_ID))
            .content(entry)
            .await
            .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(())
    }

    pub async fn is_migrated(&self) -> Result<bool> {
        let res: Option<MigrationDBEntry> = self
            .db
            .select((MIGRATIONS_TABLE, MIGRATION_ID))
            .await
            .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(res.is_some())
    }

    async fn ensure_writable(&self) -> Result<()> {
        if self.is_migrated().await? {
            return Err(Error::Migrated);
        }
        Ok(())
    }

    pub async fn dump_email_registrations(&self) -> Result<Vec<email::Registration>> {
        let res: Vec<EmailRegistrationDBEntry> = self
            .db
            .select(&self.table_registrations)
            .await
            .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(res.into_iter().map(|r| r.into()).collect())
    }

    // create a unique id by combining the node id and optional company node id
    fn id(node_id: &NodeId, company_node_id: &Option<NodeId>) -> String {
        format!(
            "{}{}",
            node_id,
            company_node_id
                .to_owned()
                .map(|cn| cn.to_string())
                .unwrap_or_default()
        )
    }
}

#[async_trait]
impl EmailConfirmationRepository for DBEmails {
    async fn insert_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
        code: &str,
    ) -> Result<()> {
        self.ensure_writable().await?;
        let entry = EmailConfirmationDBEntry {
            node_id: node_id.to_owned(),
            company_node_id: company_node_id.to_owned(),
            email: email.to_owned(),
            confirmation_code: code.to_string(),
            wrong_entries: 0,
            created_at: now(),
        };
        let id = DBEmails::id(node_id, company_node_id);

        let _res: Option<EmailConfirmationDBEntry> = self
            .db
            .upsert((&self.table_confirmations, id))
            .content(entry)
            .await
            .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(())
    }

    async fn get_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<Option<email::Confirmation>> {
        let id = DBEmails::id(node_id, company_node_id);
        let res: Option<EmailConfirmationDBEntry> = self
            .db
            .select((&self.table_confirmations, id))
            .await
            .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(res.map(|r| r.into()))
    }

    async fn record_wrong_entry_for_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<()> {
        self.ensure_writable().await?;
        let id = DBEmails::id(node_id, company_node_id);
        let table = self.table_confirmations.clone();
        let _res: Option<EmailConfirmationDBEntry> = self
            .db
            .query("UPDATE type::thing($table, $id) SET wrong_entries += 1")
            .bind(("table", table))
            .bind(("id", id))
            .await
            .map_err(|e| Error::EmailRepository(anyhow!(e)))?
            .take(0)
            .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(())
    }

    async fn remove_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<()> {
        self.ensure_writable().await?;
        let id = DBEmails::id(node_id, company_node_id);
        let _res: Option<EmailConfirmationDBEntry> = self
            .db
            .delete((&self.table_confirmations, id))
            .await
            .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(())
    }

    async fn insert_email_registration_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
        mint_signature: &Signature,
    ) -> Result<()> {
        self.ensure_writable().await?;
        let entry = EmailRegistrationDBEntry {
            node_id: node_id.to_owned(),
            company_node_id: company_node_id.to_owned(),
            email: email.to_owned(),
            mint_signature: mint_signature.to_owned(),
            created_at: now(),
        };
        let id = DBEmails::id(node_id, company_node_id);

        let _res: Option<EmailRegistrationDBEntry> = self
            .db
            .upsert((&self.table_registrations, id)) // we override, if there already was one
            .content(entry)
            .await
            .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(())
    }

    async fn get_email_registration_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<Option<email::Registration>> {
        let rid = RecordId::from((
            &self.table_registrations,
            DBEmails::id(node_id, company_node_id),
        ));
        let res: Option<EmailRegistrationDBEntry> = self
            .db
            .select(rid)
            .await
            .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(res.map(|r| r.into()))
    }
}
