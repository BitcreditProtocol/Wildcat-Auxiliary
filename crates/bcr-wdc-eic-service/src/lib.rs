use axum::{Router, extract::FromRef, routing::get};

#[derive(Clone, Debug, serde::Deserialize)]
pub struct AppConfig {}

#[derive(Clone, FromRef)]
pub struct AppController {}
impl AppController {
    pub async fn new(_cfg: AppConfig) -> Self {
        Self {}
    }
}

pub fn routes(app: AppController) -> Router {
    let web = Router::new().route("/health", get(health));
    web.with_state(app)
}

pub async fn health() -> &'static str {
    "{ \"status\": \"OK\" }"
}
