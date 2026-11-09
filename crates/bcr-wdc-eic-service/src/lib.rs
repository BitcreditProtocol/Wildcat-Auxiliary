use crate::ens::EnsRestConfig;
use axum::{
    Router,
    extract::FromRef,
    routing::{get, post},
};
use bcr_common::core::NodeId;
use bcr_wdc_shared::{
    challenge::persistence::ChallengeRepository,
    email::mailjet::{MailJetConfig, MailjetClient},
    rate_limit::RateLimiter,
};
use bitcoin::Network;
use bitcoin::secp256k1::{SECP256K1, SecretKey};
use std::sync::Arc;
use tokio::sync::Mutex;

mod email;
mod ens;
mod error;
pub mod persistence;
mod service;
mod web;

pub use service::EmailConfirmationRepository;

pub type ProdEmailClient = MailjetClient;
pub type ProdEnsClient = ens::Client;
pub type ProdService = service::Service;

/// Configuration shared by the service and the migrate binary
#[derive(Debug, serde::Deserialize)]
pub struct MainConfig {
    pub bind_address: std::net::SocketAddr,
    pub appcfg: AppConfig,
    pub log_level: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct AppConfig {
    challenges: bcr_wdc_shared::challenge::persistence::surreal::ConnectionConfig,
    pub email_confirmations: persistence::surreal::EmailConnectionConfig,
    /// PostgreSQL; `max_connections == 0` (the default) means not configured
    #[serde(default)]
    pub repository_new: bcr_wdc_shared::db::sql::DBConnConfig,
    mailjet_config: MailJetConfig,
    pub bitcoin_network: Network,
    ens: EnsRestConfig,
}

#[derive(Clone, FromRef)]
pub struct AppController {
    srvc: Arc<ProdService>,
    rate_limiter: Arc<Mutex<RateLimiter>>,
}

impl AppController {
    pub async fn new(secret_key: &SecretKey, cfg: AppConfig) -> Self {
        let cfg_clone = cfg.clone();
        let (challenge_repo, email_confirmation_repo) = init_repositories(&cfg).await;
        let email_client = ProdEmailClient::new(&cfg.mailjet_config);
        let ens_client = ProdEnsClient::new(cfg.ens.base_url);

        let mint_node_id = NodeId::new(secret_key.public_key(SECP256K1), cfg.bitcoin_network);
        let mint_private_key = secret_key.to_owned();

        let srvc = Arc::new(ProdService::new(
            challenge_repo,
            email_confirmation_repo,
            Arc::new(email_client),
            Arc::new(ens_client),
            cfg_clone,
            mint_node_id,
            mint_private_key,
        ));

        Self {
            srvc,
            rate_limiter: Arc::new(Mutex::new(RateLimiter::new())),
        }
    }
}

async fn init_repositories(
    cfg: &AppConfig,
) -> (
    Arc<dyn ChallengeRepository>,
    Arc<dyn EmailConfirmationRepository>,
) {
    let surreal_emails = persistence::surreal::DBEmails::new(cfg.email_confirmations.clone())
        .await
        .expect("email confirmations repo");
    let migrated = surreal_emails
        .is_migrated()
        .await
        .expect("migration marker");
    let pg_configured = cfg.repository_new.max_connections > 0;
    match (migrated, pg_configured) {
        (false, _) => {
            if pg_configured {
                tracing::info!(
                    "PostgreSQL configured, but not migrated yet - running on SurrealDB"
                );
            }
            let challenge_repo =
                bcr_wdc_shared::challenge::persistence::surreal::DBChallenges::new(
                    cfg.challenges.clone(),
                )
                .await
                .expect("challenges repo");
            (Arc::new(challenge_repo), Arc::new(surreal_emails))
        }
        (true, true) => {
            tracing::info!("SurrealDB is migrated - running on PostgreSQL");
            let pool = bcr_wdc_shared::db::sql::connect(&cfg.repository_new)
                .await
                .expect("postgres connection");
            bcr_wdc_shared::db::sql::run_migrations(&pool)
                .await
                .expect("postgres migrations");
            let challenge_repo =
                bcr_wdc_shared::challenge::persistence::sql::DB::from_pool(pool.clone());
            let email_confirmation_repo = persistence::sql::DB::from_pool(pool);
            (Arc::new(challenge_repo), Arc::new(email_confirmation_repo))
        }
        (true, false) => {
            panic!("SurrealDB is marked as migrated, but appcfg.repository_new is not configured")
        }
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
