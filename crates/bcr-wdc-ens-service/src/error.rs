use anyhow::Error as AnyError;
use axum::http::StatusCode;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Challenges Repository error {0}")]
    ChallengeRepository(AnyError),
    #[error("Email Notification Preferences Repository error {0}")]
    EmailNotificationPreferencesRepository(AnyError),
    #[error("SignedRequest error {0}")]
    SignedRequest(String),
    #[error("SendEmail error {0}")]
    SendEmail(String),
    #[error("Challenge error {0}")]
    Challenge(String),
    #[error("Preferences error {0}")]
    Preferences(String),
}

impl axum::response::IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        tracing::error!("Error: {}", self);
        let resp = match self {
            Error::ChallengeRepository(_) => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
            Error::EmailNotificationPreferencesRepository(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, String::new())
            }
            Error::SignedRequest(e) => (StatusCode::BAD_REQUEST, e),
            Error::SendEmail(e) => (StatusCode::BAD_REQUEST, e),
            Error::Challenge(e) => (StatusCode::BAD_REQUEST, e),
            Error::Preferences(e) => (StatusCode::BAD_REQUEST, e),
        };
        resp.into_response()
    }
}
