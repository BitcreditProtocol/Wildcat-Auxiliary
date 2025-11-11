use crate::error::{Error, Result};
use crate::{
    email_preferences::{EmailNotificationPreferences, PreferencesFlags},
    service::EmailNotificationPreferencesRepository,
};
use anyhow::anyhow;
use async_trait::async_trait;
use bcr_common::core::NodeId;
use bcr_wdc_shared::challenge::persistence::ChallengeRepository;
use bcr_wdc_shared::{TStamp, now};
use email_address::EmailAddress;
use surrealdb::{Result as SurrealResult, Surreal, engine::any::Any};
use uuid::Uuid;

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ConnectionConfig {
    pub connection: String,
    pub namespace: String,
    pub database: String,
    pub table: String,
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
pub struct EmailNotificationPreferencesDBEntry {
    pub node_id: NodeId,
    pub company_node_id: Option<NodeId>,
    pub email: EmailAddress,
    pub enabled: bool,
    pub preferences: i64,
    pub pref_token: Uuid,
    pub created_at: TStamp,
}

impl From<EmailNotificationPreferencesDBEntry> for EmailNotificationPreferences {
    fn from(value: EmailNotificationPreferencesDBEntry) -> Self {
        Self {
            node_id: value.node_id,
            company_node_id: value.company_node_id,
            email: value.email,
            enabled: value.enabled,
            preferences: PreferencesFlags::from_bits_truncate(value.preferences),
            pref_token: value.pref_token,
        }
    }
}

impl EmailNotificationPreferencesDBEntry {
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }
}

#[derive(Debug, Clone)]
pub struct DBEmailNotificationPreferences {
    db: Surreal<Any>,
    table: String,
}

impl DBEmailNotificationPreferences {
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
impl EmailNotificationPreferencesRepository for DBEmailNotificationPreferences {
    async fn get_preferences_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<Option<EmailNotificationPreferences>> {
        let id = DBEmailNotificationPreferences::id(node_id, company_node_id);
        let res: Option<EmailNotificationPreferencesDBEntry> = self
            .db
            .select((&self.table, id))
            .await
            .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?;
        Ok(res.map(|r| r.clone().into()))
    }

    async fn set_preferences_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
        enabled: bool,
        preferences: PreferencesFlags,
        pref_token: &Uuid,
    ) -> Result<()> {
        let entry = EmailNotificationPreferencesDBEntry {
            node_id: node_id.to_owned(),
            company_node_id: company_node_id.to_owned(),
            email: email.to_owned(),
            enabled,
            preferences: preferences.bits(),
            pref_token: pref_token.to_owned(),
            created_at: now(),
        };
        let id = DBEmailNotificationPreferences::id(node_id, company_node_id);

        let _res: Option<EmailNotificationPreferencesDBEntry> = self
            .db
            .upsert((&self.table, id))
            .content(entry)
            .await
            .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?;

        Ok(())
    }

    async fn get_preferences_for_token(
        &self,
        pref_token: &Uuid,
    ) -> Result<Option<EmailNotificationPreferences>> {
        let table = self.table.clone();
        let res: Option<EmailNotificationPreferencesDBEntry> = self
            .db
            .query("SELECT * FROM type::table($table) WHERE pref_token = $pref_token")
            .bind(("table", table))
            .bind(("pref_token", pref_token.to_owned()))
            .await
            .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?
            .take(0)
            .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?;

        Ok(res.map(|r| r.clone().into()))
    }

    async fn update_prefences_for_token(
        &self,
        pref_token: &Uuid,
        enabled: bool,
        preferences: PreferencesFlags,
    ) -> Result<()> {
        let table = self.table.clone();
        let _res: Option<EmailNotificationPreferencesDBEntry> = self
            .db
            .query("UPDATE type::table($table) SET enabled = $enabled, preferences = $preferences WHERE pref_token = $pref_token")
            .bind(("table", table))
            .bind(("pref_token", pref_token.to_owned()))
            .bind(("enabled", enabled))
            .bind(("preferences", preferences.bits()))
            .await
            .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?
            .take(0)
            .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?;

        Ok(())
    }
}
