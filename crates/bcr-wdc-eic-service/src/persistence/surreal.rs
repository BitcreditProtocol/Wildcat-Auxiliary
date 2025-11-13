use crate::{
    email_confirmation::EmailConfirmation,
    error::{Error, Result},
    service::EmailConfirmationRepository,
};
use anyhow::anyhow;
use async_trait::async_trait;
use bcr_common::core::NodeId;
use bcr_wdc_shared::{TStamp, challenge::persistence::ChallengeRepository, now};
use email_address::EmailAddress;
use secp256k1::schnorr::Signature;
use surrealdb::{Result as SurrealResult, Surreal, engine::any::Any};

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ConnectionConfig {
    pub connection: String,
    pub namespace: String,
    pub database: String,
    pub table: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct EmailConnectionConfig {
    pub connection: String,
    pub namespace: String,
    pub database: String,
    pub table_confirmations: String,
    pub table_registrations: String,
}

#[derive(Debug, Clone)]
pub struct DBChallenges {
    db: Surreal<Any>,
    table: String,
}

impl DBChallenges {
    pub async fn new(cfg: ConnectionConfig) -> SurrealResult<Self> {
        let db_connection = Surreal::<Any>::init();
        db_connection.connect(cfg.connection).await?;
        db_connection.use_ns(cfg.namespace).await?;
        db_connection.use_db(cfg.database).await?;
        Ok(Self {
            db: db_connection,
            table: cfg.table,
        })
    }
}

#[async_trait]
impl ChallengeRepository for DBChallenges {
    fn table(&self) -> &str {
        &self.table
    }
    fn db(&self) -> &Surreal<Any> {
        &self.db
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmailConfirmationDBEntry {
    pub node_id: NodeId,
    pub company_node_id: Option<NodeId>,
    pub email: EmailAddress,
    pub confirmation_code: String,
    pub wrong_entries: usize,
    pub created_at: TStamp,
}

impl From<EmailConfirmationDBEntry> for EmailConfirmation {
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

impl From<EmailConfirmation> for EmailConfirmationDBEntry {
    fn from(value: EmailConfirmation) -> Self {
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
    pub created_at: TStamp,
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
            .insert((&self.table_confirmations, id))
            .content(entry)
            .await
            .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(())
    }

    async fn get_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<Option<EmailConfirmation>> {
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
        let id = DBEmails::id(node_id, company_node_id);
        let table = self.table_confirmations.clone();
        let _res: Option<EmailConfirmationDBEntry> = self
            .db
            .query("UPDATE type::table($table) SET wrong_entries += 1 WHERE id = $id")
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
}
