/// This module contains Blossom-style request header parsing even though we do not require
/// service-level authentication right now. It is currently used to extract request metadata such
/// as expiration and the expected blob hash (`x` tag), which `/mirror` uses to compare the caller-
/// supplied hash against the hash of the downloaded blob. At a later point (when the Blossom part
/// of the Nostr SDK is ready) we probably want to replace this custom code with code from the SDK.
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use bitcoin::{base64::prelude::*, hashes::sha256::Hash as Sha256Hash};
use nostr::{event::Event, event::Kind};
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
        let event_json = BASE64_URL_SAFE_NO_PAD.decode(base64_token).map_err(|e| {
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
            let now = nostr::types::Timestamp::now();
            if expiration < now {
                error!("Auth token expired: expiration={}", expiration);
                return Err(AuthError::ExpiredToken);
            }
        }

        // Extract t tag
        let t_tag = event
            .tags
            .iter()
            .find(|tag| tag.kind() == "t")
            .and_then(|t| t.content().map(|s| s.to_string()));

        // Extract x tag (SHA256 hash)
        let x_tag = event
            .tags
            .iter()
            .find(|tag| tag.kind() == "x")
            .and_then(|t| {
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
    NostrAuthToken::from_header(authorization_header(headers)?)
}

fn authorization_header(headers: &axum::http::HeaderMap) -> Result<&str, AuthError> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(AuthError::BadAuthHeader)
}

/// Largest accepted base64 `Authorization` token of a NIP-98 request
const MAX_HTTP_AUTH_TOKEN_LEN: usize = 16 * 1024;
/// Largest accepted distance in seconds between a NIP-98 event's `created_at` and now
pub const HTTP_AUTH_WINDOW_SECS: u64 = 60;

/// Why a NIP-98 HTTP auth event does not authorize a request
#[derive(Debug, PartialEq, Eq)]
pub enum HttpAuthError {
    BadAuthHeader,
    MalformedToken,
    WrongKind,
    UrlMismatch,
    MethodMismatch,
    OutsideTimeWindow,
    PayloadMismatch,
    InvalidSignature,
}

impl IntoResponse for HttpAuthError {
    fn into_response(self) -> Response {
        let status = match self {
            HttpAuthError::BadAuthHeader | HttpAuthError::MalformedToken => StatusCode::BAD_REQUEST,
            _ => StatusCode::UNAUTHORIZED,
        };
        (status, format!("{self:?}")).into_response()
    }
}

/// Verifies a NIP-98 `Authorization: Nostr <base64 event>` header for one HTTP request and
/// returns the key that signed it.
///
/// The event must be of kind 27235, its `u` tag must equal `url`, its `method` tag must equal
/// `method`, its `created_at` must lie within [`HTTP_AUTH_WINDOW_SECS`] of `now` in either
/// direction, its `payload` tag must be the hex SHA-256 of `body` whenever `body` is not empty,
/// and its id and signature must verify. The token is accepted in standard or URL-safe base64.
pub fn verify_http_auth(
    headers: &axum::http::HeaderMap,
    url: &nostr::types::Url,
    method: &axum::http::Method,
    body: &[u8],
    now: nostr::types::Timestamp,
) -> Result<nostr::key::PublicKey, HttpAuthError> {
    let header = authorization_header(headers).map_err(|_| HttpAuthError::BadAuthHeader)?;
    let token = match header.split_once(' ') {
        Some(("Nostr", token)) if !token.is_empty() => token.trim(),
        _ => return Err(HttpAuthError::BadAuthHeader),
    };
    if token.len() > MAX_HTTP_AUTH_TOKEN_LEN {
        return Err(HttpAuthError::MalformedToken);
    }
    let event_json = BASE64_STANDARD
        .decode(token)
        .or_else(|_| BASE64_URL_SAFE_NO_PAD.decode(token))
        .map_err(|_| HttpAuthError::MalformedToken)?;
    let event: Event =
        serde_json::from_slice(&event_json).map_err(|_| HttpAuthError::MalformedToken)?;

    if event.kind != Kind::HttpAuth {
        return Err(HttpAuthError::WrongKind);
    }
    let tag_value = |name: &str| {
        event
            .tags
            .iter()
            .find(|tag| tag.kind() == name)
            .and_then(|tag| tag.content())
    };
    let signed_url = tag_value("u").and_then(|u| nostr::types::Url::parse(u).ok());
    if signed_url.as_ref() != Some(url) {
        return Err(HttpAuthError::UrlMismatch);
    }
    if tag_value("method") != Some(method.as_str()) {
        return Err(HttpAuthError::MethodMismatch);
    }
    if event.created_at.as_secs().abs_diff(now.as_secs()) > HTTP_AUTH_WINDOW_SECS {
        return Err(HttpAuthError::OutsideTimeWindow);
    }
    if !body.is_empty() {
        let body_hash = <Sha256Hash as bitcoin::hashes::Hash>::hash(body).to_string();
        if tag_value("payload") != Some(body_hash.as_str()) {
            return Err(HttpAuthError::PayloadMismatch);
        }
    }
    event
        .verify()
        .map_err(|_| HttpAuthError::InvalidSignature)?;
    Ok(event.pubkey)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{
        event::{EventBuilder, FinalizeEvent, Tag},
        key::Keys,
        types::Timestamp,
    };

    #[test]
    fn test_base64_url_compatibility() {
        let sequences: [&[u8]; 4] = [
            // length % 3 == 0: no padding needed
            b"abc",
            // length % 3 == 1: would need "==" padding
            &[0x00],
            // length % 3 == 2: would need "=" padding
            b"{}",
            // bytes mapping to the URL-safe alphabet characters '-' and '_'
            &[0xfb, 0xff, 0xbf, 0xfe],
        ];

        for seq in sequences {
            let reference = base64_url::encode(seq);
            let encoded = BASE64_URL_SAFE_NO_PAD.encode(seq);
            assert_eq!(encoded, reference, "encode mismatch for {seq:?}");
            assert_eq!(
                BASE64_URL_SAFE_NO_PAD.decode(&reference).unwrap(),
                seq,
                "decode of base64_url output failed for {seq:?}"
            );
            assert_eq!(
                base64_url::decode(&encoded).unwrap(),
                seq,
                "base64_url decode of our output failed for {seq:?}"
            );
        }
    }

    fn create_test_keys() -> Keys {
        Keys::generate()
    }

    #[test]
    fn test_from_header_valid() {
        let keys = create_test_keys();
        let event = EventBuilder::new(Kind::from(24242), "test")
            .finalize(&keys)
            .unwrap();

        let event_json = serde_json::to_string(&event).unwrap();
        let base64_token = BASE64_URL_SAFE_NO_PAD.encode(&event_json);
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
            .tag(Tag::expiration(past_time))
            .finalize(&keys)
            .unwrap();

        let event_json = serde_json::to_string(&event).unwrap();
        let base64_token = BASE64_URL_SAFE_NO_PAD.encode(&event_json);

        let result = NostrAuthToken::from_base64(&base64_token);
        assert!(matches!(result, Err(AuthError::ExpiredToken)));
    }

    #[test]
    fn test_extract_x_tag() {
        let keys = create_test_keys();
        let hash_str = "0000000000000000000000000000000000000000000000000000000000000001";

        let event = EventBuilder::new(Kind::from(24242), "test")
            .tag(Tag::hashtag("mirror"))
            .tag(Tag::parse(["x", hash_str]).unwrap())
            .finalize(&keys)
            .unwrap();

        let event_json = serde_json::to_string(&event).unwrap();
        let base64_token = BASE64_URL_SAFE_NO_PAD.encode(&event_json);

        let token = NostrAuthToken::from_base64(&base64_token).unwrap();

        assert!(token.x_tag.is_some());
        assert_eq!(token.x_tag.unwrap().to_string(), hash_str);
    }

    #[test]
    fn test_missing_x_tag() {
        let keys = create_test_keys();

        let event = EventBuilder::new(Kind::from(24242), "test")
            .finalize(&keys)
            .unwrap();

        let event_json = serde_json::to_string(&event).unwrap();
        let base64_token = BASE64_URL_SAFE_NO_PAD.encode(&event_json);

        let token = NostrAuthToken::from_base64(&base64_token).unwrap();

        assert!(token.x_tag.is_none());
        assert!(matches!(token.require_x_tag(), Err(AuthError::MissingXTag)));
    }
}
