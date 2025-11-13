use axum::{
    Router,
    extract::FromRef,
    routing::{get, post},
};
use std::sync::Arc;

mod email_preferences;
mod error;
mod persistence;
mod service;
mod web;

pub type ProdChallengeRepository = persistence::surreal::DBChallenges;
pub type ProdEmailConfirmationRepository = persistence::surreal::DBEmailNotificationPreferences;
pub type ProdService = service::Service;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct AppConfig {
    challenges: persistence::surreal::ConnectionConfig,
    email_notification_preferences: persistence::surreal::ConnectionConfig,
}

#[derive(Clone, FromRef)]
pub struct AppController {
    srvc: Arc<ProdService>,
}

impl AppController {
    pub async fn new(cfg: AppConfig) -> Self {
        let email_notification_preferences_repo =
            ProdEmailConfirmationRepository::new(cfg.email_notification_preferences)
                .await
                .expect("email notification preferences repo");
        let challenge_repo = ProdChallengeRepository::new(cfg.challenges)
            .await
            .expect("challenges repo");

        let srvc = Arc::new(ProdService::new(
            Arc::new(challenge_repo),
            Arc::new(email_notification_preferences_repo),
        ));

        Self { srvc }
    }
}

pub fn routes(app: AppController) -> Router {
    let web = Router::new()
        // only available internally to be called by eic-service
        .route("/v1/email/preferences", post(web::set_email_preferences))
        .route("/v1/challenge", post(web::challenge))
        .route("/health", get(web::health));
    web.with_state(app)
}
