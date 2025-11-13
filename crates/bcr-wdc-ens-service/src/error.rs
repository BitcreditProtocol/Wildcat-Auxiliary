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
}

impl axum::response::IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        tracing::error!("Error: {}", self);
        let resp = match self {
            Error::ChallengeRepository(_) => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
            Error::EmailNotificationPreferencesRepository(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, String::new())
            }
        };
        resp.into_response()
    }
}
