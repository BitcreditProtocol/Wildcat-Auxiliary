use anyhow::Result;
use async_trait::async_trait;
use bcr_common::core::NodeId;

use crate::{TStamp, challenge::Challenge};

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
}
