/// This module contains Blossom-style request header parsing even though we do not require
/// service-level authentication right now. It is currently used to extract request metadata such
/// as expiration and the expected blob hash (`x` tag), which `/mirror` uses to compare the caller-
/// supplied hash against the hash of the downloaded blob. At a later point (when the Blossom part
/// of the Nostr SDK is ready) we probably want to replace this custom code with code from the SDK.
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use nostr::{Kind, TagKind, event::Event, hashes::sha256::Hash as Sha256Hash};
use tracing::error;

/// Errors that can occur during auth token validation
#[derive(Debug)]
pub enum AuthError {
    BadAuthHeader,
    MalformedToken,
    ExpiredToken,
    MissingXTag,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::BadAuthHeader => {
                (StatusCode::BAD_REQUEST, "Bad Authorization header format")
            }
            AuthError::MalformedToken => (StatusCode::BAD_REQUEST, "Malformed auth token"),
            AuthError::ExpiredToken => (StatusCode::UNAUTHORIZED, "Auth token expired"),
            AuthError::MissingXTag => (StatusCode::BAD_REQUEST, "Missing x tag in auth event"),
        };
        (status, message).into_response()
    }
}

/// Parsed Nostr authorization token with extracted fields
#[derive(Debug, Clone)]
pub struct NostrAuthToken {
    #[allow(dead_code)]
    pub event: Event,
    #[allow(dead_code)]
    pub kind: Kind,
    #[allow(dead_code)]
    pub t_tag: Option<String>,
    pub x_tag: Option<Sha256Hash>,
}

impl NostrAuthToken {
    /// Parse an Authorization header value in the format "Nostr <base64url>"
    pub fn from_header(auth_header: &str) -> Result<Self, AuthError> {
        let parts: Vec<&str> = auth_header.split_whitespace().collect();
        if parts.len() != 2 || parts[0] != "Nostr" {
            return Err(AuthError::BadAuthHeader);
        }

        Self::from_base64(parts[1])
    }

    /// Parse a base64url-encoded Nostr event
    pub fn from_base64(base64_token: &str) -> Result<Self, AuthError> {
        let event_json = base64_url::decode(base64_token).map_err(|e| {
            error!("Failed to decode base64 auth token: {}", e);
            AuthError::MalformedToken
        })?;

        let event: Event = serde_json::from_slice(&event_json).map_err(|e| {
            error!("Failed to parse Nostr event JSON: {}", e);
            AuthError::MalformedToken
        })?;

        Self::from_event(event)
    }

    /// Validate a Nostr event and extract relevant fields
    pub fn from_event(event: Event) -> Result<Self, AuthError> {
        // Check if event is expired
        if let Some(expiration) = event.tags.expiration() {
            let now = nostr::Timestamp::now();
            if *expiration < now {
                error!("Auth token expired: expiration={}", expiration);
                return Err(AuthError::ExpiredToken);
            }
        }

        // Extract t tag
        let t_tag = event
            .tags
            .find(TagKind::from("t"))
            .and_then(|t| t.content().map(|s| s.to_string()));

        // Extract x tag (SHA256 hash)
        let x_tag = event.tags.find(TagKind::from("x")).and_then(|t| {
            t.content()
                .and_then(|content| content.parse::<Sha256Hash>().ok())
        });

        Ok(Self {
            kind: event.kind,
            t_tag,
            x_tag,
            event,
        })
    }

    /// Get the x tag, returning an error if missing
    pub fn require_x_tag(&self) -> Result<Sha256Hash, AuthError> {
        self.x_tag.ok_or(AuthError::MissingXTag)
    }
}

/// Extractor for NostrAuthToken from Authorization header
pub fn extract_auth_token(headers: &axum::http::HeaderMap) -> Result<NostrAuthToken, AuthError> {
    let auth_header = headers
        .get("authorization")
        .or_else(|| headers.get("Authorization"))
        .and_then(|v| v.to_str().ok())
        .ok_or(AuthError::BadAuthHeader)?;

    NostrAuthToken::from_header(auth_header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{Timestamp, event::EventBuilder, key::Keys};

    fn create_test_keys() -> Keys {
        Keys::generate()
    }

    #[test]
    fn test_from_header_valid() {
        let keys = create_test_keys();
        let event = EventBuilder::new(Kind::from(24242), "test")
            .sign_with_keys(&keys)
            .unwrap();

        let event_json = serde_json::to_string(&event).unwrap();
        let base64_token = base64_url::encode(&event_json);
        let auth_header = format!("Nostr {}", base64_token);

        let result = NostrAuthToken::from_header(&auth_header);
        assert!(result.is_ok());

        let token = result.unwrap();
        assert_eq!(token.kind, Kind::from(24242));
    }

    #[test]
    fn test_from_header_bad_format() {
        let result = NostrAuthToken::from_header("Bearer token123");
        assert!(matches!(result, Err(AuthError::BadAuthHeader)));

        let result = NostrAuthToken::from_header("invalid");
        assert!(matches!(result, Err(AuthError::BadAuthHeader)));
    }

    #[test]
    fn test_expired_token() {
        let keys = create_test_keys();
        let past_time = Timestamp::now() - 3600; // 1 hour ago

        let event = EventBuilder::new(Kind::from(24242), "test")
            .tag(nostr::Tag::expiration(past_time))
            .sign_with_keys(&keys)
            .unwrap();

        let event_json = serde_json::to_string(&event).unwrap();
        let base64_token = base64_url::encode(&event_json);

        let result = NostrAuthToken::from_base64(&base64_token);
        assert!(matches!(result, Err(AuthError::ExpiredToken)));
    }

    #[test]
    fn test_extract_x_tag() {
        let keys = create_test_keys();
        let hash_str = "0000000000000000000000000000000000000000000000000000000000000001";

        let event = EventBuilder::new(Kind::from(24242), "test")
            .tag(nostr::Tag::hashtag("mirror"))
            .tag(nostr::Tag::parse(["x", hash_str]).unwrap())
            .sign_with_keys(&keys)
            .unwrap();

        let event_json = serde_json::to_string(&event).unwrap();
        let base64_token = base64_url::encode(&event_json);

        let token = NostrAuthToken::from_base64(&base64_token).unwrap();

        assert!(token.x_tag.is_some());
        assert_eq!(token.x_tag.unwrap().to_string(), hash_str);
    }

    #[test]
    fn test_missing_x_tag() {
        let keys = create_test_keys();

        let event = EventBuilder::new(Kind::from(24242), "test")
            .sign_with_keys(&keys)
            .unwrap();

        let event_json = serde_json::to_string(&event).unwrap();
        let base64_token = base64_url::encode(&event_json);

        let token = NostrAuthToken::from_base64(&base64_token).unwrap();

        assert!(token.x_tag.is_none());
        assert!(matches!(token.require_x_tag(), Err(AuthError::MissingXTag)));
    }
}
