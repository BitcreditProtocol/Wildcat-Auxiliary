use crate::{
    AppConfig,
    email::build_email_notification_message,
    email_preferences::{EmailNotificationPreferences, PreferencesFlags},
    error::{Error, Result},
    template::{self, PreferencesContext, PreferencesContextContent, preferences_as_content_flags},
};
use async_trait::async_trait;
use bcr_common::core::NodeId;
use bcr_wdc_shared::{
    challenge::{Challenge, persistence::ChallengeRepository},
    email::mailjet::EmailClient,
};
use email_address::EmailAddress;
use secp256k1::schnorr::Signature;
use std::sync::Arc;
use tracing::{error, warn};
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
        enabled: bool,
        preferences: PreferencesFlags,
        pref_token: &Uuid,
    ) -> Result<()>;

    async fn get_preferences_for_token(
        &self,
        pref_token: &Uuid,
    ) -> Result<Option<EmailNotificationPreferences>>;

    async fn update_prefences_for_token(
        &self,
        pref_token: &Uuid,
        enabled: bool,
        preferences: PreferencesFlags,
    ) -> Result<()>;
}

pub struct Service {
    challenge_repo: Arc<dyn ChallengeRepository>,
    email_notification_repo: Arc<dyn EmailNotificationPreferencesRepository>,
    email_client: Arc<dyn EmailClient>,
    cfg: AppConfig,
}

impl Service {
    pub fn new(
        challenge_repo: Arc<dyn ChallengeRepository>,
        email_notification_repo: Arc<dyn EmailNotificationPreferencesRepository>,
        email_client: Arc<dyn EmailClient>,
        cfg: AppConfig,
    ) -> Self {
        Self {
            challenge_repo,
            email_notification_repo,
            email_client,
            cfg,
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
                        entry.enabled,
                        entry.preferences,
                        &entry.pref_token,
                    )
                    .await?
            }
            Ok(None) => {
                // No entry yet - set one with default preferences
                let pref_token = Uuid::new_v4();
                self.email_notification_repo
                    .set_preferences_for_node_id(
                        node_id,
                        company_node_id,
                        email,
                        true,                        // enable by default
                        PreferencesFlags::default(), // with default flags
                        &pref_token,
                    )
                    .await?
            }
            Err(e) => return Err(e),
        };
        Ok(())
    }

    pub async fn send_email(
        &self,
        receiver_node_id: &NodeId,
        receiver_company_node_id: &Option<NodeId>,
        kind: &str,
        id: &str,
    ) -> Result<()> {
        let email_preferences = match self
            .email_notification_repo
            .get_preferences_for_node_id(receiver_node_id, receiver_company_node_id)
            .await
        {
            Ok(Some(pref)) => pref,
            Ok(None) => return Ok(()), // no preferences - do nothing
            Err(e) => {
                warn!(
                    "Error fetching email preferences for {receiver_node_id} / {receiver_company_node_id:?} for sending email: {e}"
                );
                return Ok(());
            }
        };

        let notification_type = match PreferencesFlags::from_name(kind) {
            Some(nt) => nt,
            None => {
                return Err(Error::SendEmail("Invalid Kind".into()));
            }
        };

        // disabled - don't send
        if !email_preferences.enabled {
            return Ok(());
        }

        // prefernce not enabled - don't send
        if !email_preferences.preferences.contains(notification_type) {
            return Ok(());
        }

        let email_msg = match build_email_notification_message(
            &self.cfg.mailjet_config.logo_url,
            &self.cfg.mailjet_config.sender,
            &email_preferences.email,
            &notification_type.to_title(),
            &notification_type.to_link(&self.cfg.app_url, id),
            &self.build_preferences_link(&email_preferences.pref_token),
        ) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Could not create notification mail: {e}");
                return Err(Error::SendEmail("Invalid Mail".into()));
            }
        };

        if let Err(e) = self.email_client.send(email_msg).await {
            error!("Notification send mail error: {e}");
            return Err(Error::SendEmail("Email sending".into()));
        }

        Ok(())
    }

    pub async fn get_preferences_link(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<url::Url> {
        let email_preferences = match self
            .email_notification_repo
            .get_preferences_for_node_id(node_id, company_node_id)
            .await
        {
            Ok(Some(pref)) => pref,
            Ok(None) => return Err(Error::Preferences("No Preferences found".into())),
            Err(e) => {
                warn!(
                    "Error fetching email preferences for {node_id} / {company_node_id:?} for getting preferences link: {e}"
                );
                return Err(Error::Preferences("Preferences Error".into()));
            }
        };

        Ok(self.build_preferences_link(&email_preferences.pref_token))
    }

    pub async fn get_preferences(
        &self,
        pref_token: &Uuid,
    ) -> Result<(&'static str, PreferencesContext)> {
        // check email preferences exist
        let email_preferences = match self
            .email_notification_repo
            .get_preferences_for_token(pref_token)
            .await
        {
            Ok(Some(p)) => p,
            Ok(None) => {
                return Err(Error::Preferences("No Preferences".into()));
            }
            Err(e) => {
                error!("notification update preferences invalid token: {e}");
                return Err(Error::Preferences("Preferences Error".into()));
            }
        };

        let ctx = PreferencesContext {
            content: PreferencesContextContent {
                enabled: email_preferences.enabled,
                pref_token: email_preferences.pref_token,
                email: email_preferences.email,
                node_id: email_preferences.node_id,
                company_node_id: email_preferences.company_node_id,
                flags: preferences_as_content_flags(email_preferences.preferences),
            },
            title: "Email Preferences".to_owned(),
            logo_link: self.cfg.mailjet_config.logo_url.clone(),
        };
        Ok((template::PREFERENCES_TEMPLATE, ctx))
    }

    pub async fn update_preferences(
        &self,
        pref_token: &Uuid,
        enabled: bool,
        updated_preferences: Option<PreferencesFlags>,
    ) -> Result<()> {
        // check email preferences exist
        let email_preferences = match self
            .email_notification_repo
            .get_preferences_for_token(pref_token)
            .await
        {
            Ok(Some(p)) => p,
            Ok(None) => {
                return Err(Error::Preferences("No Preferences".into()));
            }
            Err(e) => {
                error!("notification update preferences invalid token: {e}");
                return Err(Error::Preferences("Preferences Error".into()));
            }
        };

        let prefs_to_set = match updated_preferences {
            Some(p) => p,
            None => email_preferences.preferences,
        };

        self.email_notification_repo
            .update_prefences_for_token(pref_token, enabled, prefs_to_set)
            .await?;

        Ok(())
    }

    fn build_preferences_link(&self, pref_token: &Uuid) -> url::Url {
        self.cfg
            .host_url
            .join(&format!("/email/preferences/{}", pref_token))
            .expect("email notification mail")
    }

    pub async fn check_challenge(
        &self,
        node_id: &NodeId,
        signed_challenge: &Signature,
    ) -> Result<bool> {
        // check if challenge exists
        let Some((challenge, created_at)) = self
            .challenge_repo
            .get_challenge_for_node_id(node_id)
            .await
            .map_err(Error::ChallengeRepository)?
        else {
            return Err(Error::Challenge("No Challenge Found".into()));
        };
        challenge
            .check(node_id, signed_challenge, created_at)
            .map_err(|e| Error::Challenge(e.to_string()))?;

        // delete consumed challenge
        if let Err(e) = self
            .challenge_repo
            .remove_challenge_for_node_id(node_id)
            .await
        {
            warn!("Couldn't delete consumed challenge for {node_id}: {e}");
        }

        Ok(true)
    }

    pub fn get_logo_link(&self) -> url::Url {
        self.cfg.mailjet_config.logo_url.clone()
    }
}
