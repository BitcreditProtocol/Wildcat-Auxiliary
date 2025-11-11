use crate::ens::EnsRestConfig;
use axum::{
    Router,
    extract::FromRef,
    routing::{get, post},
};
use bcr_common::core::NodeId;
use bcr_wdc_shared::email::mailjet::{MailJetConfig, MailjetClient};
use bitcoin::Network;
use secp256k1::{SECP256K1, SecretKey};
use std::sync::Arc;

mod email;
mod email_confirmation;
mod ens;
mod error;
mod persistence;
mod service;
mod web;

pub type ProdChallengeRepository = persistence::surreal::DBChallenges;
pub type ProdEmailConfirmationRepository = persistence::surreal::DBEmails;
pub type ProdEmailClient = MailjetClient;
pub type ProdEnsClient = ens::Client;
pub type ProdService = service::Service;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct AppConfig {
    challenges: persistence::surreal::ConnectionConfig,
    email_confirmations: persistence::surreal::EmailConnectionConfig,
    mailjet_config: MailJetConfig,
    pub bitcoin_network: Network,
    ens: EnsRestConfig,
}

#[derive(Clone, FromRef)]
pub struct AppController {
    srvc: Arc<ProdService>,
}

impl AppController {
    pub async fn new(secret_key: &SecretKey, cfg: AppConfig) -> Self {
        let cfg_clone = cfg.clone();
        let challenge_repo = ProdChallengeRepository::new(cfg.challenges)
            .await
            .expect("challenges repo");
        let email_confirmation_repo = ProdEmailConfirmationRepository::new(cfg.email_confirmations)
            .await
            .expect("challenges repo");
        let email_client = ProdEmailClient::new(&cfg.mailjet_config);
        let ens_client = ProdEnsClient::new(cfg.ens.base_url);

        let mint_node_id = NodeId::new(secret_key.public_key(SECP256K1), cfg.bitcoin_network);
        let mint_private_key = secret_key.to_owned();

        let srvc = Arc::new(ProdService::new(
            Arc::new(challenge_repo),
            Arc::new(email_confirmation_repo),
            Arc::new(email_client),
            Arc::new(ens_client),
            cfg_clone,
            mint_node_id,
            mint_private_key,
        ));

        Self { srvc }
    }
}

pub fn routes(app: AppController) -> Router {
    let web = Router::new()
        // health
        .route("/health", get(web::health))
        // external API endpoints
        .route("/v1/email/register", post(web::email_register))
        .route("/v1/email/confirm", post(web::email_confirm))
        .route("/v1/challenge", post(web::challenge));
    web.with_state(app)
}
