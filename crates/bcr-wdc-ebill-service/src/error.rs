// ----- standard library imports
// ----- extra library imports
use axum::http::StatusCode;
use bcr_ebill_api::service::{self, bill_service};
use bcr_ebill_core::protocol::ProtocolValidationError;
use thiserror::Error;
// ----- end imports

/// Generic result type
pub type Result<T> = std::result::Result<T, Error>;

/// Generic error type
#[derive(Debug, Error)]
pub enum Error {
    /// all errors originating from the bcr API service layer
    #[error("convert error: {0}")]
    Convert(#[from] crate::convert::Error),

    /// all errors originating from the bcr API service layer
    #[error("Service error: {0}")]
    Service(#[from] service::Error),

    /// all errors originating from the bcr API bill service layer
    #[error("Bill Service error: {0}")]
    BillService(#[from] service::bill_service::Error),

    /// all errors originating from the bcr API notification service layer
    #[error("Notification Service error: {0}")]
    NotificationService(#[from] bcr_ebill_api::service::transport_service::Error),

    /// all errors originating from validation
    #[error("Validation error: {0}")]
    Validation(#[from] bcr_ebill_core::application::ValidationError),

    /// all errors originating from creating an identity, if an identity already exists
    #[error("Identity already exists")]
    IdentityAlreadyExists,

    /// all errors originating from creating an identity, if an identity already exists
    #[error("Identity type")]
    IdentityType,

    /// all errors originating from invalid mnemonics
    #[error("Invalid Mnemonic")]
    InvalidMnemonic,

    /// all errors originating from identity conversion
    #[error("Invalid identity")]
    IdentityConversion,

    /// all errors originating from File Downloading
    #[error("File Download Error: {0}")]
    FileDownload(String),

    /// all errors originating from validating and decrypting a shared bill
    #[error("Shared Bill Error: {0}")]
    SharedBill(String),

    #[error("Protocol Validation error: {0}")]
    ProtocolValidation(#[from] ProtocolValidationError),
}

impl axum::response::IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        tracing::error!("Error: {self}");
        match self {
            Error::Convert(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
            Error::Service(e) => ServiceError(e).into_response(),
            Error::BillService(e) => BillServiceError(e).into_response(),
            Error::NotificationService(e) => ServiceError(e.into()).into_response(),
            Error::Validation(e) => ValidationError(e).into_response(),
            Error::FileDownload(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
            Error::IdentityType => (
                StatusCode::BAD_REQUEST,
                String::from("invalid identity type"),
            )
                .into_response(),
            Error::IdentityConversion => (
                StatusCode::INTERNAL_SERVER_ERROR,
                String::from("invalid identity"),
            )
                .into_response(),
            Error::InvalidMnemonic => (
                StatusCode::BAD_REQUEST,
                String::from("invalid bip39 mnemonic"),
            )
                .into_response(),
            Error::SharedBill(e) => (StatusCode::BAD_REQUEST, e).into_response(),
            Error::IdentityAlreadyExists => (
                StatusCode::BAD_REQUEST,
                String::from("Identity already exists"),
            )
                .into_response(),
            Error::ProtocolValidation(e) => {
                (StatusCode::BAD_REQUEST, e.to_string()).into_response()
            }
        }
    }
}

pub struct ServiceError(bcr_ebill_api::service::Error);

impl axum::response::IntoResponse for ServiceError {
    fn into_response(self) -> axum::response::Response {
        match self.0 {
            bcr_ebill_api::service::Error::Json(_) => {
                (StatusCode::BAD_REQUEST, self.0.to_string()).into_response()
            }
            bcr_ebill_api::service::Error::NotFound => {
                (StatusCode::NOT_FOUND, "not found".to_string()).into_response()
            }
            bcr_ebill_api::service::Error::BillService(e) => BillServiceError(e).into_response(),
            bcr_ebill_api::service::Error::ExternalApi(_)
            | service::Error::TransportService(_)
            | bcr_ebill_api::service::Error::CryptoUtil(_)
            | bcr_ebill_api::service::Error::Persistence(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, String::new()).into_response()
            }
            service::Error::Validation(validation_error) => {
                ValidationError(validation_error).into_response()
            }
            service::Error::Protocol(protocol_error) => {
                ProtocolError(protocol_error).into_response()
            }
        }
    }
}

pub struct BillServiceError(bill_service::Error);

impl axum::response::IntoResponse for BillServiceError {
    fn into_response(self) -> axum::response::Response {
        match self.0 {
            bill_service::Error::Validation(validation_err) => {
                ValidationError(validation_err).into_response()
            }
            bill_service::Error::NotFound => {
                (StatusCode::NOT_FOUND, "not found".to_string()).into_response()
            }
            bill_service::Error::Persistence(_)
            | bill_service::Error::ExternalApi(_)
            | bill_service::Error::Cryptography(_)
            | bill_service::Error::Notification(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, String::new()).into_response()
            }
            bill_service::Error::Protocol(protocol_error) => {
                ProtocolError(protocol_error).into_response()
            }
            bill_service::Error::Json(_) => {
                (StatusCode::BAD_REQUEST, self.0.to_string()).into_response()
            }
        }
    }
}

pub struct ValidationError(bcr_ebill_core::application::ValidationError);

impl axum::response::IntoResponse for ValidationError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::BAD_REQUEST, self.0.to_string()).into_response()
    }
}

pub struct ProtocolError(bcr_ebill_core::protocol::ProtocolError);

impl axum::response::IntoResponse for ProtocolError {
    fn into_response(self) -> axum::response::Response {
        match self.0 {
            bcr_ebill_core::protocol::ProtocolError::Serialization(_)
            | bcr_ebill_core::protocol::ProtocolError::Deserialization(_)
            | bcr_ebill_core::protocol::ProtocolError::Crypto(_)
            | bcr_ebill_core::protocol::ProtocolError::Blockchain(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, String::new()).into_response()
            }
            bcr_ebill_core::protocol::ProtocolError::Validation(protocol_validation_error) => (
                StatusCode::BAD_REQUEST,
                protocol_validation_error.to_string(),
            )
                .into_response(),
        }
    }
}
