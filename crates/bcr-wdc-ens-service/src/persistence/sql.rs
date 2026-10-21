use crate::error::{Error, Result};
use crate::{
    email_preferences::{EmailNotificationPreferences, PreferencesFlags},
    service::EmailNotificationPreferencesRepository,
};
use anyhow::anyhow;
use async_trait::async_trait;
use bcr_common::core::NodeId;
use bcr_wdc_shared::now;
use email_address::EmailAddress;
use sqlx::{PgPool, types::Json};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "version", content = "data")]
enum Blob {
    V1(BlobV1),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BlobV1 {
    email: String,
}

#[derive(sqlx::FromRow, Debug)]
struct Row {
    node_id: String,
    company_node_id: String,
    pref_token: Uuid,
    enabled: bool,
    preferences: i64,
    blob: Json<Blob>,
}

impl TryFrom<Row> for EmailNotificationPreferences {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self> {
        let node_id = NodeId::from_str(&row.node_id)
            .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?;
        let company_node_id = if row.company_node_id.is_empty() {
            None
        } else {
            Some(
                NodeId::from_str(&row.company_node_id)
                    .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?,
            )
        };
        let email = match &row.blob.0 {
            Blob::V1(blob) => EmailAddress::from_str(&blob.email)
                .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?,
        };
        Ok(Self {
            node_id,
            company_node_id,
            email,
            enabled: row.enabled,
            preferences: PreferencesFlags::from_bits_truncate(row.preferences),
            pref_token: row.pref_token,
        })
    }
}

#[derive(Debug, Clone)]
pub struct DB {
    pool: PgPool,
}

impl DB {
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    fn company_key(company_node_id: &Option<NodeId>) -> String {
        match company_node_id {
            Some(cn) => cn.to_string(),
            None => String::default(),
        }
    }
}

#[async_trait]
impl EmailNotificationPreferencesRepository for DB {
    async fn get_preferences_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<Option<EmailNotificationPreferences>> {
        let row = sqlx::query_as!(
            Row,
            r#"SELECT node_id, company_node_id, pref_token, enabled, preferences, blob AS "blob: Json<Blob>"
               FROM email_notification_preferences WHERE node_id = $1 AND company_node_id = $2"#,
            node_id.to_string(),
            DB::company_key(company_node_id),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?;
        row.map(EmailNotificationPreferences::try_from).transpose()
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
        let blob = Json(Blob::V1(BlobV1 {
            email: email.to_string(),
        }));
        sqlx::query!(
            r#"INSERT INTO email_notification_preferences
               (node_id, company_node_id, pref_token, enabled, preferences, created_at, blob)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               ON CONFLICT (node_id, company_node_id) DO UPDATE
               SET pref_token = EXCLUDED.pref_token, enabled = EXCLUDED.enabled,
                   preferences = EXCLUDED.preferences, created_at = EXCLUDED.created_at,
                   blob = EXCLUDED.blob"#,
            node_id.to_string(),
            DB::company_key(company_node_id),
            pref_token,
            enabled,
            preferences.bits(),
            now(),
            blob as _,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?;
        Ok(())
    }

    async fn get_preferences_for_token(
        &self,
        pref_token: &Uuid,
    ) -> Result<Option<EmailNotificationPreferences>> {
        let row = sqlx::query_as!(
            Row,
            r#"SELECT node_id, company_node_id, pref_token, enabled, preferences, blob AS "blob: Json<Blob>"
               FROM email_notification_preferences WHERE pref_token = $1"#,
            pref_token,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?;
        row.map(EmailNotificationPreferences::try_from).transpose()
    }

    async fn update_prefences_for_token(
        &self,
        pref_token: &Uuid,
        enabled: bool,
        preferences: PreferencesFlags,
    ) -> Result<()> {
        sqlx::query!(
            r#"UPDATE email_notification_preferences SET enabled = $2, preferences = $3
               WHERE pref_token = $1"#,
            pref_token,
            enabled,
            preferences.bits(),
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::EmailNotificationPreferencesRepository(anyhow!(e)))?;
        Ok(())
    }
}
