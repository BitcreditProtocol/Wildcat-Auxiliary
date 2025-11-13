use crate::{
    AppConfig,
    email::build_email_confirmation_message,
    email_confirmation::EmailConfirmation,
    error::{Error, Result},
};
use async_trait::async_trait;
use bcr_common::core::NodeId;
use bcr_wdc_shared::{
    challenge::{Challenge, persistence::ChallengeRepository},
    email::mailjet::EmailClient,
    now, signature,
    wire::MintSignature,
};
use email_address::EmailAddress;
use secp256k1::{SecretKey, schnorr::Signature};
use std::sync::Arc;
use tracing::warn;

#[async_trait]
pub trait EnsClient: Send + Sync {
    async fn set_email_preferences(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
    ) -> Result<()>;
}

#[async_trait]
pub trait EmailConfirmationRepository: Send + Sync {
    async fn insert_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
        code: &str,
    ) -> Result<()>;

    async fn get_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<Option<EmailConfirmation>>;

    async fn record_wrong_entry_for_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<()>;

    async fn remove_confirmation_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
    ) -> Result<()>;

    async fn insert_email_registration_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
        mint_signature: &Signature,
    ) -> Result<()>;
}

pub struct Service {
    challenge_repo: Arc<dyn ChallengeRepository>,
    email_confirmation_repo: Arc<dyn EmailConfirmationRepository>,
    email_client: Arc<dyn EmailClient>,
    ens_client: Arc<dyn EnsClient>,
    cfg: AppConfig,
    mint_node_id: NodeId,
    mint_private_key: SecretKey,
}

impl Service {
    pub fn new(
        challenge_repo: Arc<dyn ChallengeRepository>,
        email_confirmation_repo: Arc<dyn EmailConfirmationRepository>,
        email_client: Arc<dyn EmailClient>,
        ens_client: Arc<dyn EnsClient>,
        cfg: AppConfig,
        mint_node_id: NodeId,
        mint_private_key: SecretKey,
    ) -> Self {
        Self {
            challenge_repo,
            email_confirmation_repo,
            email_client,
            ens_client,
            cfg,
            mint_node_id,
            mint_private_key,
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

    pub async fn register_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
        signed_challenge: &Signature,
    ) -> Result<()> {
        self.check_challenge(node_id, signed_challenge).await?;

        let confirmation_code = Self::create_confirmation_code();

        // persist
        self.email_confirmation_repo
            .insert_confirmation_email_for_node_id(
                node_id,
                company_node_id,
                email,
                &confirmation_code,
            )
            .await?;

        // build confirmation email
        let confirmation_msg = build_email_confirmation_message(
            &self.cfg.mailjet_config.logo_url,
            &self.cfg.mailjet_config.sender,
            email.as_ref(),
            &confirmation_code,
        )
        .map_err(Error::EmailClient)?;

        // send email
        self.email_client
            .send(confirmation_msg)
            .await
            .map_err(Error::EmailClient)?;

        Ok(())
    }

    pub async fn confirm_email_for_node_id(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        confirmation_code: &str,
    ) -> Result<(MintSignature, Signature, NodeId)> {
        // get confirmation email state
        let Some(confirmation) = self
            .email_confirmation_repo
            .get_confirmation_email_for_node_id(node_id, company_node_id)
            .await?
        else {
            return Err(Error::Confirmation("No Confirmation Found".into()));
        };

        // check if it's expired
        if confirmation.is_expired() {
            return Err(Error::Confirmation("Confirmation Expired".into()));
        }

        // check if it has too many retries
        if confirmation.has_too_many_retries() {
            return Err(Error::Confirmation("Too Many Wrong Entries".into()));
        }

        // check if the confirmation code is correct
        if !confirmation.matches_confirmation_code(confirmation_code) {
            // if not, increment wrong tries
            if let Err(e) = self
                .email_confirmation_repo
                .record_wrong_entry_for_confirmation_email_for_node_id(node_id, company_node_id)
                .await
            {
                warn!(
                    "Couldn't record wrong confirmation code entry for {node_id} / {company_node_id:?}: {e}"
                );
            }
            return Err(Error::Confirmation("Wrong Code".into()));
        }

        // calculate mint signature
        let (mint_signature_payload, mint_signature) =
            self.create_mint_signature(node_id, company_node_id, &confirmation.email)?;

        // persist in table_registrations with mint_signature
        self.email_confirmation_repo
            .insert_email_registration_for_node_id(
                node_id,
                company_node_id,
                &confirmation.email,
                &mint_signature,
            )
            .await?;

        // set email preferences in ens service
        if let Err(e) = self
            .ens_client
            .set_email_preferences(node_id, company_node_id, &confirmation.email)
            .await
        {
            warn!(
                "Couldn't set email preferences after successful confirmation for {node_id} / {company_node_id:?}: {e}"
            );
        }

        // remove consumed confirmation email
        if let Err(e) = self
            .email_confirmation_repo
            .remove_confirmation_email_for_node_id(node_id, company_node_id)
            .await
        {
            warn!(
                "Couldn't delete email confirmation after successful confirmation for {node_id} / {company_node_id:?}: {e}"
            );
        }

        Ok((
            mint_signature_payload,
            mint_signature,
            self.mint_node_id.clone(),
        ))
    }

    /// Sign the sha256 hash of the MintSignature serialized with borsh with the mint key
    fn create_mint_signature(
        &self,
        node_id: &NodeId,
        company_node_id: &Option<NodeId>,
        email: &EmailAddress,
    ) -> Result<(MintSignature, Signature)> {
        let sig_payload = MintSignature {
            node_id: node_id.to_owned(),
            company_node_id: company_node_id.to_owned(),
            email: email.to_owned(),
            created_at: now(),
        };
        let borshed = borsh::to_vec(&sig_payload).map_err(|e| Error::Signature(e.to_string()))?;
        let signature = signature::sign_payload(&borshed, &self.mint_private_key);
        Ok((sig_payload, signature))
    }

    async fn check_challenge(
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

        // check if challenge timed out
        if now()
            > (created_at
                .checked_add_signed(challenge.ttl())
                .expect("safe to add seconds"))
        {
            return Err(Error::Challenge("Challenge Timed Out".into()));
        }

        let x_only = node_id.pub_key().x_only_public_key().0;

        // check if challenge is valid
        match signature::verify_signature(
            &challenge
                .decode()
                .map_err(|_| Error::Challenge("Invalid Challenge".into()))?,
            signed_challenge,
            &x_only,
        ) {
            Ok(true) => {
                // delete consumed challenge
                if let Err(e) = self
                    .challenge_repo
                    .remove_challenge_for_node_id(node_id)
                    .await
                {
                    warn!("Couldn't delete consumed challenge for {node_id}: {e}");
                }
            }
            Ok(false) => {
                return Err(Error::Challenge("Invalid Challenge".into()));
            }
            Err(e) => {
                warn!("Couldn't check challenge: {e}");
                return Err(Error::Challenge("Error Checking Challenge".into()));
            }
        };

        Ok(true)
    }

    /// create random 6 digit code
    fn create_confirmation_code() -> String {
        rand::random_range(100_000..1_000_000).to_string()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_confirmation_code() {
        for _ in 0..1000 {
            let code = Service::create_confirmation_code();
            assert_eq!(code.len(), 6);
            assert!(code.chars().all(|c| c.is_ascii_digit()));
        }
    }
}
