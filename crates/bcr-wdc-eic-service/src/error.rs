use anyhow::Error as AnyError;
use axum::http::StatusCode;
use bitcoin::base58;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Challenges Repository error {0}")]
    ChallengeRepository(AnyError),
    #[error("Ens Client error {0}")]
    EnsClient(AnyError),
    #[error("Emails Repository error {0}")]
    EmailRepository(AnyError),
    #[error("Base58 encoding error {0}")]
    Encoding(#[from] base58::InvalidCharacterError),
    #[error("Challenge error {0}")]
    Challenge(String),
    #[error("Confirmation error {0}")]
    Confirmation(String),
    #[error("Signature error {0}")]
    Signature(String),
    #[error("EmailClient error {0}")]
    EmailClient(AnyError),
    #[error("SignedRequest error {0}")]
    SignedRequest(String),
    #[error("RateLimit error")]
    RateLimit,
}

impl axum::response::IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        tracing::error!("Error: {}", self);
        let resp = match self {
            Error::ChallengeRepository(_) => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
            Error::EmailRepository(_) => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
            Error::EnsClient(_) => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
            Error::EmailClient(_) => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
            Error::Encoding(_) => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
            Error::Challenge(e) => (StatusCode::BAD_REQUEST, e),
            Error::Signature(_) => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
            Error::Confirmation(e) => (StatusCode::BAD_REQUEST, e),
            Error::SignedRequest(e) => (StatusCode::BAD_REQUEST, e),
            Error::RateLimit => (
                StatusCode::TOO_MANY_REQUESTS,
                String::from("Please try again later"),
            ),
        };
        resp.into_response()
    }
}
