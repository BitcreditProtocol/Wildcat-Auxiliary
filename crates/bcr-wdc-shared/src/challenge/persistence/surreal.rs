use anyhow::{Result, anyhow};
use async_trait::async_trait;
use bcr_common::core::NodeId;
use surrealdb::{Surreal, engine::any::Any};

use crate::{
    TStamp,
    challenge::{Challenge, persistence::ChallengeRepository},
    now,
};

type SurrealResult<T> = std::result::Result<T, Box<surrealdb::Error>>;

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
    async fn insert_challenge_for_node_id(
        &self,
        node_id: &NodeId,
        challenge: &Challenge,
    ) -> Result<()> {
        let entry = ChallengeDBEntry {
            node_id: node_id.to_owned(),
            challenge: challenge.to_string(),
            created_at: now(),
        };

        let _res: Option<ChallengeDBEntry> = self
            .db
            .upsert((&self.table, node_id.to_string()))
            .content(entry)
            .await
            .map_err(|e| anyhow!(e))?;
        Ok(())
    }

    async fn get_challenge_for_node_id(
        &self,
        node_id: &NodeId,
    ) -> Result<Option<(Challenge, TStamp)>> {
        let res: Option<ChallengeDBEntry> = self
            .db
            .select((&self.table, node_id.to_string()))
            .await
            .map_err(|e| anyhow!(e))?;
        Ok(res.map(|r| (r.clone().into(), r.created_at)))
    }

    async fn remove_challenge_for_node_id(&self, node_id: &NodeId) -> Result<()> {
        let _res: Option<ChallengeDBEntry> = self
            .db
            .delete((&self.table, node_id.to_string()))
            .await
            .map_err(|e| anyhow!(e))?;
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChallengeDBEntry {
    pub node_id: NodeId,
    pub challenge: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: TStamp,
}

impl From<ChallengeDBEntry> for Challenge {
    fn from(value: ChallengeDBEntry) -> Self {
        Challenge::from(value.challenge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{
        Network,
        secp256k1::{SECP256K1, SecretKey},
    };

    async fn init_db() -> DBChallenges {
        DBChallenges::new(ConnectionConfig {
            connection: String::from("mem://"),
            namespace: String::from("test"),
            database: String::from("test"),
            table: String::from("challenges"),
        })
        .await
        .expect("mem db")
    }

    fn random_node_id() -> NodeId {
        let sk = SecretKey::new(&mut bitcoin::secp256k1::rand::thread_rng());
        NodeId::new(sk.public_key(SECP256K1), Network::Testnet)
    }

    #[tokio::test]
    async fn get_missing_challenge() {
        let db = init_db().await;
        let res = db
            .get_challenge_for_node_id(&random_node_id())
            .await
            .unwrap();
        assert!(res.is_none());
    }

    #[tokio::test]
    async fn insert_and_get_challenge() {
        let db = init_db().await;
        let node_id = random_node_id();
        let challenge = Challenge::new();
        db.insert_challenge_for_node_id(&node_id, &challenge)
            .await
            .unwrap();
        let (stored, _) = db
            .get_challenge_for_node_id(&node_id)
            .await
            .unwrap()
            .expect("challenge");
        assert_eq!(stored, challenge);
    }

    #[tokio::test]
    async fn insert_overwrites_challenge() {
        let db = init_db().await;
        let node_id = random_node_id();
        db.insert_challenge_for_node_id(&node_id, &Challenge::new())
            .await
            .unwrap();
        let second = Challenge::new();
        db.insert_challenge_for_node_id(&node_id, &second)
            .await
            .unwrap();
        let (stored, _) = db
            .get_challenge_for_node_id(&node_id)
            .await
            .unwrap()
            .expect("challenge");
        assert_eq!(stored, second);
    }

    #[tokio::test]
    async fn remove_challenge() {
        let db = init_db().await;
        let node_id = random_node_id();
        db.insert_challenge_for_node_id(&node_id, &Challenge::new())
            .await
            .unwrap();
        db.remove_challenge_for_node_id(&node_id).await.unwrap();
        let res = db.get_challenge_for_node_id(&node_id).await.unwrap();
        assert!(res.is_none());
    }
}
