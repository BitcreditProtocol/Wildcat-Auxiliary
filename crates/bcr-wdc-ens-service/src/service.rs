use crate::{
    email_preferences::{EmailNotificationPreferences, PreferencesFlags},
    error::{Error, Result},
};
use async_trait::async_trait;
use bcr_common::core::NodeId;
use bcr_wdc_shared::challenge::{Challenge, persistence::ChallengeRepository};
use email_address::EmailAddress;
use std::sync::Arc;
use uuid::Uuid;

#[async_trait]
pub trait EmailNotificationPreferencesRepository: Send + Sync {
    async fn get_preferences_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<Option<EmailNotificationPreferences>>;

    async fn set_preferences_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
        preferences: PreferencesFlags,
        token: &Uuid,
    ) -> Result<()>;
}

pub struct Service {
    challenge_repo: Arc<dyn ChallengeRepository>,
    email_notification_repo: Arc<dyn EmailNotificationPreferencesRepository>,
}

impl Service {
    pub fn new(
        challenge_repo: Arc<dyn ChallengeRepository>,
        email_notification_repo: Arc<dyn EmailNotificationPreferencesRepository>,
    ) -> Self {
        Self {
            challenge_repo,
            email_notification_repo,
        }
    }

    pub async fn create_challenge_for_node_id(&self, node_id: &NodeId) -> Result<Challenge> {
        let challenge = Challenge::new();
        self.challenge_repo
            .insert_challenge_for_node_id(node_id, &challenge)
            .await
            .map_err(Error::ChallengeRepository)?;
        Ok(challenge)
    }

    pub async fn set_email_notification_preferences(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
    ) -> Result<()> {
        match self
            .email_notification_repo
            .get_preferences_for_node_id(node_id, company_node_id)
            .await
        {
            Ok(Some(entry)) => {
                // There is already an entry - update email address and keep the rest of the settings as they are
                self.email_notification_repo
                    .set_preferences_for_node_id(
                        &entry.node_id,
                        &entry.company_node_id,
                        email, // only update the email
                        entry.preferences,
                        &entry.token,
                    )
                    .await?
            }
            Ok(None) => {
                // No entry yet - set one with default preferences
                let token = Uuid::new_v4();
                self.email_notification_repo
                    .set_preferences_for_node_id(
                        node_id,
                        company_node_id,
                        email,
                        PreferencesFlags::default(),
                        &token,
                    )
                    .await?
            }
            Err(e) => return Err(e),
        };
        Ok(())
    }
}
