use crate::{
    email,
    error::{Error, Result},
    service::EmailConfirmationRepository,
};
use anyhow::anyhow;
use async_trait::async_trait;
use bcr_common::core::NodeId;
use bcr_wdc_shared::now;
use bitcoin::secp256k1::schnorr::Signature;
use email_address::EmailAddress;
use sqlx::{PgPool, types::Json};
use std::str::FromStr;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "version", content = "data")]
enum ConfirmationBlob {
    V1(ConfirmationBlobV1),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ConfirmationBlobV1 {
    email: String,
    confirmation_code: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "version", content = "data")]
enum RegistrationBlob {
    V1(RegistrationBlobV1),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RegistrationBlobV1 {
    email: String,
    mint_signature: String,
}

#[derive(Debug, Clone)]
pub struct DB {
    pool: PgPool,
}

impl DB {
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    // company_node_id is part of the primary key: personal entries use ''
    fn company_key(company_node_id: &Option<NodeId>) -> String {
        company_node_id
            .as_ref()
            .map(|cn| cn.to_string())
            .unwrap_or_default()
    }
}

fn parse_email(email: &str) -> Result<EmailAddress> {
    EmailAddress::from_str(email).map_err(|e| Error::EmailRepository(anyhow!(e)))
}

#[async_trait]
impl EmailConfirmationRepository for DB {
    async fn insert_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
        code: &str,
    ) -> Result<()> {
        let blob = Json(ConfirmationBlob::V1(ConfirmationBlobV1 {
            email: email.to_string(),
            confirmation_code: code.to_string(),
        }));
        sqlx::query!(
            r#"INSERT INTO email_confirmations (node_id, company_node_id, wrong_entries, created_at, blob)
               VALUES ($1, $2, 0, $3, $4)
               ON CONFLICT (node_id, company_node_id) DO UPDATE
               SET wrong_entries = 0, created_at = EXCLUDED.created_at, blob = EXCLUDED.blob"#,
            node_id.to_string(),
            DB::company_key(company_node_id),
            now(),
            blob as _,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(())
    }

    async fn get_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<Option<email::Confirmation>> {
        let row = sqlx::query!(
            r#"SELECT wrong_entries, created_at, blob AS "blob: Json<ConfirmationBlob>"
               FROM email_confirmations WHERE node_id = $1 AND company_node_id = $2"#,
            node_id.to_string(),
            DB::company_key(company_node_id),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        let Some(row) = row else {
            return Ok(None);
        };
        let ConfirmationBlob::V1(blob) = row.blob.0;
        Ok(Some(email::Confirmation {
            node_id: node_id.to_owned(),
            company_node_id: company_node_id.to_owned(),
            email: parse_email(&blob.email)?,
            confirmation_code: blob.confirmation_code,
            wrong_entries: usize::try_from(row.wrong_entries)
                .map_err(|e| Error::EmailRepository(anyhow!(e)))?,
            created_at: row.created_at,
        }))
    }

    async fn record_wrong_entry_for_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<()> {
        sqlx::query!(
            r#"UPDATE email_confirmations SET wrong_entries = wrong_entries + 1
               WHERE node_id = $1 AND company_node_id = $2"#,
            node_id.to_string(),
            DB::company_key(company_node_id),
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(())
    }

    async fn remove_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<()> {
        sqlx::query!(
            "DELETE FROM email_confirmations WHERE node_id = $1 AND company_node_id = $2",
            node_id.to_string(),
            DB::company_key(company_node_id),
        )
        .execute(&self.pool)
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
        let blob = Json(RegistrationBlob::V1(RegistrationBlobV1 {
            email: email.to_string(),
            mint_signature: mint_signature.to_string(),
        }));
        // we override, if there already was one
        sqlx::query!(
            r#"INSERT INTO email_registrations (node_id, company_node_id, created_at, blob)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (node_id, company_node_id) DO UPDATE
               SET created_at = EXCLUDED.created_at, blob = EXCLUDED.blob"#,
            node_id.to_string(),
            DB::company_key(company_node_id),
            now(),
            blob as _,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        Ok(())
    }

    async fn get_email_registration_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<Option<email::Registration>> {
        let row = sqlx::query!(
            r#"SELECT created_at, blob AS "blob: Json<RegistrationBlob>"
               FROM email_registrations WHERE node_id = $1 AND company_node_id = $2"#,
            node_id.to_string(),
            DB::company_key(company_node_id),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::EmailRepository(anyhow!(e)))?;
        let Some(row) = row else {
            return Ok(None);
        };
        let RegistrationBlob::V1(blob) = row.blob.0;
        Ok(Some(email::Registration {
            node_id: node_id.to_owned(),
            company_node_id: company_node_id.to_owned(),
            email: parse_email(&blob.email)?,
            mint_signature: Signature::from_str(&blob.mint_signature)
                .map_err(|e| Error::EmailRepository(anyhow!(e)))?,
            created_at: row.created_at,
        }))
    }
}
