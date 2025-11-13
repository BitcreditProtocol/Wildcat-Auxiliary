use axum::{
    Router,
    extract::FromRef,
    routing::{get, post},
};
use bcr_wdc_shared::{
    email::mailjet::{MailJetConfig, MailjetClient},
    rate_limit::RateLimiter,
};
use std::sync::Arc;
use tokio::sync::Mutex;

mod email;
mod email_preferences;
mod error;
mod persistence;
mod service;
mod template;
mod web;

pub type ProdChallengeRepository = persistence::surreal::DBChallenges;
pub type ProdEmailConfirmationRepository = persistence::surreal::DBEmailNotificationPreferences;
pub type ProdService = service::Service;
pub type ProdEmailClient = MailjetClient;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct AppConfig {
    host_url: url::Url,
    app_url: url::Url,
    challenges: persistence::surreal::ConnectionConfig,
    email_notification_preferences: persistence::surreal::ConnectionConfig,
    mailjet_config: MailJetConfig,
}

#[derive(Clone, FromRef)]
pub struct AppController {
    srvc: Arc<ProdService>,
    rate_limiter: Arc<Mutex<RateLimiter>>,
}

impl AppController {
    pub async fn new(cfg: AppConfig) -> Self {
        let cfg_clone = cfg.clone();
        let email_notification_preferences_repo =
            ProdEmailConfirmationRepository::new(cfg.email_notification_preferences)
                .await
                .expect("email notification preferences repo");
        let challenge_repo = ProdChallengeRepository::new(cfg.challenges)
            .await
            .expect("challenges repo");
        let email_client = ProdEmailClient::new(&cfg.mailjet_config);

        let srvc = Arc::new(ProdService::new(
            Arc::new(challenge_repo),
            Arc::new(email_notification_preferences_repo),
            Arc::new(email_client),
            cfg_clone,
        ));

        Self {
            srvc,
            rate_limiter: Arc::new(Mutex::new(RateLimiter::new())),
        }
    }
}

pub fn routes(app: AppController) -> Router {
    let web = Router::new()
        // internal endpoint
        .route("/v1/email/preferences", post(web::set_email_preferences))
        // public endpoints
        .route("/v1/email/send", post(web::send_email))
        .route(
            "/v1/email/preferences/link",
            post(web::get_email_preferences_link),
        )
        .route("/v1/challenge", post(web::challenge))
        // HTML endpoints
        .route("/email/preferences/{token}", get(web::preferences))
        .route("/email/update_preferences", post(web::update_preferences))
        // health
        .route("/health", get(web::health));
    web.with_state(app)
}
