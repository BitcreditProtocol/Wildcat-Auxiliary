use anyhow::Result;
use async_trait::async_trait;
use bcr_common::core::NodeId;

use crate::{TStamp, challenge::Challenge};

pub mod sql;
pub mod surreal;

#[async_trait]
pub trait ChallengeRepository: Send + Sync {
    async fn insert_challenge_for_node_id(
        &self,
        node_id: &NodeId,
        challenge: &Challenge,
    ) -> Result<()>;

    async fn get_challenge_for_node_id(
        &self,
        node_id: &NodeId,
    ) -> Result<Option<(Challenge, TStamp)>>;

    async fn remove_challenge_for_node_id(&self, node_id: &NodeId) -> Result<()>;

    async fn clean_expired(&self, deadline: TStamp) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{
        Network,
        secp256k1::{SECP256K1, SecretKey},
    };

    async fn init_surreal_db() -> surreal::DBChallenges {
        surreal::DBChallenges::new(surreal::ConnectionConfig {
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
    async fn test_challenges_get_missing() {
        let db = init_surreal_db().await;
        challenges_get_missing(db).await;
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_challenges_get_missing_sql(pool: ::sqlx::PgPool) {
        challenges_get_missing(sql::DB::from_pool(pool)).await;
    }
    async fn challenges_get_missing(db: impl ChallengeRepository) {
        let res = db
            .get_challenge_for_node_id(&random_node_id())
            .await
            .unwrap();
        assert!(res.is_none());
    }

    #[tokio::test]
    async fn test_challenges_insert_and_get() {
        let db = init_surreal_db().await;
        challenges_insert_and_get(db).await;
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_challenges_insert_and_get_sql(pool: ::sqlx::PgPool) {
        challenges_insert_and_get(sql::DB::from_pool(pool)).await;
    }
    async fn challenges_insert_and_get(db: impl ChallengeRepository) {
        let node_id = random_node_id();
        let challenge = Challenge::new();
        let before = crate::now();
        db.insert_challenge_for_node_id(&node_id, &challenge)
            .await
            .unwrap();
        let (stored, created_at) = db
            .get_challenge_for_node_id(&node_id)
            .await
            .unwrap()
            .expect("challenge");
        assert_eq!(stored, challenge);
        // surreal stores rfc3339, which may drop sub-second precision
        assert!(created_at >= before - time::Duration::seconds(1));
    }

    #[tokio::test]
    async fn test_challenges_insert_overwrites() {
        let db = init_surreal_db().await;
        challenges_insert_overwrites(db).await;
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_challenges_insert_overwrites_sql(pool: ::sqlx::PgPool) {
        challenges_insert_overwrites(sql::DB::from_pool(pool)).await;
    }
    async fn challenges_insert_overwrites(db: impl ChallengeRepository) {
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
    async fn test_challenges_clean_expired() {
        let db = init_surreal_db().await;
        challenges_clean_expired(db).await;
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_challenges_clean_expired_sql(pool: ::sqlx::PgPool) {
        challenges_clean_expired(sql::DB::from_pool(pool)).await;
    }
    async fn challenges_clean_expired(db: impl ChallengeRepository) {
        let node_id = random_node_id();
        db.insert_challenge_for_node_id(&node_id, &Challenge::new())
            .await
            .unwrap();
        // created after the deadline: kept
        db.clean_expired(crate::now() - time::Duration::minutes(1))
            .await
            .unwrap();
        assert!(
            db.get_challenge_for_node_id(&node_id)
                .await
                .unwrap()
                .is_some()
        );
        // created before the deadline: removed
        db.clean_expired(crate::now() + time::Duration::minutes(1))
            .await
            .unwrap();
        assert!(
            db.get_challenge_for_node_id(&node_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_challenges_remove() {
        let db = init_surreal_db().await;
        challenges_remove(db).await;
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_challenges_remove_sql(pool: ::sqlx::PgPool) {
        challenges_remove(sql::DB::from_pool(pool)).await;
    }
    async fn challenges_remove(db: impl ChallengeRepository) {
        let node_id = random_node_id();
        let other = random_node_id();
        db.insert_challenge_for_node_id(&node_id, &Challenge::new())
            .await
            .unwrap();
        db.insert_challenge_for_node_id(&other, &Challenge::new())
            .await
            .unwrap();
        db.remove_challenge_for_node_id(&node_id).await.unwrap();
        assert!(
            db.get_challenge_for_node_id(&node_id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db.get_challenge_for_node_id(&other)
                .await
                .unwrap()
                .is_some()
        );
    }
}
