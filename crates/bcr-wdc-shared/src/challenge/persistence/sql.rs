use anyhow::{Result, anyhow};
use async_trait::async_trait;
use bcr_common::core::NodeId;
use sqlx::{PgPool, types::Json};

use crate::{
    TStamp,
    challenge::{Challenge, persistence::ChallengeRepository},
    now,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "version", content = "data")]
enum Blob {
    V1(BlobV1),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BlobV1 {
    challenge: String,
}

impl From<&Challenge> for Blob {
    fn from(value: &Challenge) -> Self {
        Self::V1(BlobV1 {
            challenge: value.to_string(),
        })
    }
}

impl From<Blob> for Challenge {
    fn from(value: Blob) -> Self {
        match value {
            Blob::V1(v1) => Challenge::from(v1.challenge),
        }
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
}

#[async_trait]
impl ChallengeRepository for DB {
    async fn insert_challenge_for_node_id(
        &self,
        node_id: &NodeId,
        challenge: &Challenge,
    ) -> Result<()> {
        let blob = Json(Blob::from(challenge));
        sqlx::query!(
            r#"INSERT INTO challenges (node_id, created_at, blob) VALUES ($1, $2, $3)
               ON CONFLICT (node_id) DO UPDATE SET created_at = EXCLUDED.created_at, blob = EXCLUDED.blob"#,
            node_id.to_string(),
            now(),
            blob as _,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| anyhow!(e))?;
        Ok(())
    }

    async fn get_challenge_for_node_id(
        &self,
        node_id: &NodeId,
    ) -> Result<Option<(Challenge, TStamp)>> {
        let row = sqlx::query!(
            r#"SELECT created_at, blob AS "blob: Json<Blob>" FROM challenges WHERE node_id = $1"#,
            node_id.to_string(),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| anyhow!(e))?;
        Ok(row.map(|r| (r.blob.0.into(), r.created_at)))
    }

    async fn remove_challenge_for_node_id(&self, node_id: &NodeId) -> Result<()> {
        sqlx::query!(
            "DELETE FROM challenges WHERE node_id = $1",
            node_id.to_string()
        )
        .execute(&self.pool)
        .await
        .map_err(|e| anyhow!(e))?;
        Ok(())
    }

    async fn clean_expired(&self, deadline: TStamp) -> Result<()> {
        sqlx::query!("DELETE FROM challenges WHERE created_at < $1", deadline)
            .execute(&self.pool)
            .await
            .map_err(|e| anyhow!(e))?;
        Ok(())
    }
}
