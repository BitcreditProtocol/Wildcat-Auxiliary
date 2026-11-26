pub mod device_store;
pub mod fcm;
pub mod queue;

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use nostr::{
    key::PublicKey,
    types::{Timestamp, Url},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::{AppState, auth::verify_http_auth};
use device_store::DeviceStoreApi;

/// Largest accepted `/push` request body
pub const MAX_BODY_BYTES: usize = 8 * 1024;
/// Largest accepted push token
const MAX_TOKEN_LEN: usize = 4096;

/// Platform a push token belongs to; a key holds at most one token per platform
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Android,
    Ios,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Android => "android",
            Platform::Ios => "ios",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "android" => Some(Platform::Android),
            "ios" => Some(Platform::Ios),
            _ => None,
        }
    }
}

/// Body of `PUT /push` and `DELETE /push`; the key is the signer of the NIP-98 auth event
///
/// # Example
/// ```json
/// {"platform": "android", "token": "fcm-registration-token"}
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceRequest {
    pub platform: Platform,
    pub token: String,
}

/// A push token registered for a key on one platform
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub key: PublicKey,
    pub platform: Platform,
    pub token: String,
}

/// The absolute URL NIP-98 auth events for device registration must name in their `u` tag
pub fn push_url(host_url: &Url) -> Option<Url> {
    if host_url.cannot_be_a_base() {
        return None;
    }
    let mut url = host_url.clone();
    let path = format!("{}/push", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Some(url)
}

/// `PUT /push` registers or replaces the signer's token for a platform; `DELETE /push` removes
/// it. Both answer 204, also when there was nothing to remove.
pub async fn handle_device(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    apply_device_request(
        state.device_store.as_ref(),
        &state.cfg.host_url,
        &method,
        &headers,
        &body,
        Timestamp::now(),
    )
    .await
}

async fn apply_device_request(
    store: &dyn DeviceStoreApi,
    host_url: &Url,
    method: &Method,
    headers: &HeaderMap,
    body: &[u8],
    now: Timestamp,
) -> Response {
    let url = match push_url(host_url) {
        Some(url) => url,
        None => {
            error!("Invalid push URL for host {host_url}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let key = match verify_http_auth(headers, &url, method, body, now) {
        Ok(key) => key,
        Err(e) => {
            info!("Rejected {method} /push: {e:?}");
            return e.into_response();
        }
    };
    let request: DeviceRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("Invalid body: {e}")).into_response(),
    };
    if request.token.is_empty()
        || request.token.len() > MAX_TOKEN_LEN
        || request.token.contains('\0')
    {
        return (StatusCode::BAD_REQUEST, "Invalid token").into_response();
    }
    let device = Device {
        key,
        platform: request.platform,
        token: request.token,
    };

    let result = match *method {
        Method::PUT => store.upsert(&device).await,
        Method::DELETE => store.remove(&device).await.map(|_| ()),
        _ => return StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };
    match result {
        Ok(()) => {
            info!("{method} /push for {} on {}", key, device.platform.as_str());
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            error!("Device store failed on {method} /push: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::HttpAuthError;
    use axum::http::header::AUTHORIZATION;
    use bitcoin::base64::prelude::*;
    use bitcoin::hashes::{Hash, sha256};
    use device_store::MemoryDeviceStore;
    use nostr::{
        event::{EventBuilder, FinalizeEvent, Kind, Tag},
        key::Keys,
    };

    #[derive(Deserialize)]
    struct Fixture {
        pubkey: PublicKey,
        host_url: Url,
        now: u64,
        requests: Vec<FixtureRequest>,
    }

    #[derive(Deserialize)]
    struct FixtureRequest {
        method: String,
        url: Url,
        body: String,
        authorization: String,
    }

    fn fixture() -> Fixture {
        serde_json::from_str(include_str!("../../fixtures/push_auth.json")).unwrap()
    }

    fn fixture_request(fixture: &Fixture, method: &Method) -> (HeaderMap, Vec<u8>) {
        let request = fixture
            .requests
            .iter()
            .find(|r| r.method == method.as_str())
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, request.authorization.parse().unwrap());
        (headers, request.body.as_bytes().to_vec())
    }

    fn host() -> Url {
        Url::parse("https://relay.example.com").unwrap()
    }

    fn signed(keys: &Keys, method: &str, url: &str, body: &[u8], created_at: u64) -> HeaderMap {
        let mut tags = vec![
            Tag::parse(["u", url]).unwrap(),
            Tag::parse(["method", method]).unwrap(),
        ];
        if !body.is_empty() {
            let payload = sha256::Hash::hash(body).to_string();
            tags.push(Tag::parse(["payload", payload.as_str()]).unwrap());
        }
        let event = EventBuilder::new(Kind::HttpAuth, "")
            .tags(tags)
            .custom_created_at(Timestamp::from_secs(created_at))
            .finalize(keys)
            .unwrap();
        let token = BASE64_STANDARD.encode(serde_json::to_vec(&event).unwrap());
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, format!("Nostr {token}").parse().unwrap());
        headers
    }

    fn body(platform: &str, token: &str) -> Vec<u8> {
        format!(r#"{{"platform":"{platform}","token":"{token}"}}"#).into_bytes()
    }

    const NOW: u64 = 1_767_225_600;

    async fn send(
        store: &MemoryDeviceStore,
        method: Method,
        headers: &HeaderMap,
        body: &[u8],
        now: u64,
    ) -> StatusCode {
        apply_device_request(
            store,
            &host(),
            &method,
            headers,
            body,
            Timestamp::from_secs(now),
        )
        .await
        .status()
    }

    #[test]
    fn push_device_url_keeps_host_path() {
        let host = Url::parse("https://relay.example.com/a").unwrap();
        let slashed = Url::parse("https://relay.example.com/a/").unwrap();
        assert_eq!(
            push_url(&host).unwrap().as_str(),
            "https://relay.example.com/a/push"
        );
        assert_eq!(push_url(&slashed), push_url(&host));
        for deep in [
            "https://relay.example.com///",
            "https://relay.example.com/a//",
        ] {
            let url = push_url(&Url::parse(deep).unwrap()).unwrap();
            assert!(!url.path().contains("//"), "{url}");
        }
    }

    #[tokio::test]
    async fn push_device_rejects_nul_in_token() {
        let store = MemoryDeviceStore::default();
        let body = br#"{"platform":"android","token":"a\u0000b"}"#;
        let headers = signed(
            &Keys::generate(),
            "PUT",
            "https://relay.example.com/push",
            body,
            NOW,
        );
        assert_eq!(
            send(&store, Method::PUT, &headers, body, NOW).await,
            StatusCode::BAD_REQUEST
        );
        assert!(store.tokens().is_empty());
    }

    #[test]
    fn push_device_fixture_names_the_push_url() {
        let fixture = fixture();
        for request in &fixture.requests {
            assert_eq!(request.url, push_url(&fixture.host_url).unwrap());
        }
    }

    #[tokio::test]
    async fn push_device_fixture_registers_and_unregisters() {
        let fixture = fixture();
        let store = MemoryDeviceStore::default();

        let (headers, put_body) = fixture_request(&fixture, &Method::PUT);
        let status = apply_device_request(
            &store,
            &fixture.host_url,
            &Method::PUT,
            &headers,
            &put_body,
            Timestamp::from_secs(fixture.now),
        )
        .await
        .status();
        assert_eq!(status, StatusCode::NO_CONTENT);
        let request: DeviceRequest = serde_json::from_slice(&put_body).unwrap();
        assert_eq!(
            store.tokens(),
            vec![(fixture.pubkey, request.platform, request.token)]
        );

        let (headers, delete_body) = fixture_request(&fixture, &Method::DELETE);
        let status = apply_device_request(
            &store,
            &fixture.host_url,
            &Method::DELETE,
            &headers,
            &delete_body,
            Timestamp::from_secs(fixture.now),
        )
        .await
        .status();
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(store.tokens().is_empty());
    }

    #[test]
    fn push_device_time_window_is_inclusive_at_sixty_seconds() {
        let keys = Keys::generate();
        let body = body("ios", "apns");
        let headers = signed(&keys, "PUT", "https://relay.example.com/push", &body, NOW);
        let url = Url::parse("https://relay.example.com/push").unwrap();
        for now in [NOW - 60, NOW + 60] {
            let result = verify_http_auth(
                &headers,
                &url,
                &Method::PUT,
                &body,
                Timestamp::from_secs(now),
            );
            assert_eq!(result, Ok(keys.public_key()));
        }
        for now in [NOW - 61, NOW + 61] {
            let result = verify_http_auth(
                &headers,
                &url,
                &Method::PUT,
                &body,
                Timestamp::from_secs(now),
            );
            assert_eq!(result, Err(HttpAuthError::OutsideTimeWindow));
        }
    }

    #[tokio::test]
    async fn push_device_rejects_created_at_sixty_one_seconds_ahead() {
        let store = MemoryDeviceStore::default();
        let body = body("android", "fcm");
        let headers = signed(
            &Keys::generate(),
            "PUT",
            "https://relay.example.com/push",
            &body,
            NOW + 61,
        );
        let status = send(&store, Method::PUT, &headers, &body, NOW).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(store.tokens().is_empty());
    }

    #[tokio::test]
    async fn push_device_rejects_url_of_another_host() {
        let store = MemoryDeviceStore::default();
        let body = body("android", "fcm");
        for url in [
            "https://other.example.com/push",
            "https://relay.example.com.evil.net/push",
            "https://relay.example.com/other",
        ] {
            let headers = signed(&Keys::generate(), "PUT", url, &body, NOW);
            let status = send(&store, Method::PUT, &headers, &body, NOW).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{url}");
        }
        assert!(store.tokens().is_empty());
    }

    #[tokio::test]
    async fn push_device_rejects_wrong_method() {
        let store = MemoryDeviceStore::default();
        let keys = Keys::generate();
        let body = body("android", "fcm");
        let put = signed(&keys, "PUT", "https://relay.example.com/push", &body, NOW);
        assert_eq!(
            send(&store, Method::PUT, &put, &body, NOW).await,
            StatusCode::NO_CONTENT
        );

        let status = send(&store, Method::DELETE, &put, &body, NOW).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(store.tokens().len(), 1);
    }

    #[tokio::test]
    async fn push_device_rejects_body_not_signed() {
        let store = MemoryDeviceStore::default();
        let signed_body = body("android", "victim");
        let headers = signed(
            &Keys::generate(),
            "PUT",
            "https://relay.example.com/push",
            &signed_body,
            NOW,
        );
        let status = send(
            &store,
            Method::PUT,
            &headers,
            &body("android", "attacker"),
            NOW,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(store.tokens().is_empty());
    }

    #[tokio::test]
    async fn push_device_rejects_forged_signer() {
        let store = MemoryDeviceStore::default();
        let fixture = fixture();
        let (headers, body) = fixture_request(&fixture, &Method::PUT);
        let token = headers[AUTHORIZATION].to_str().unwrap()["Nostr ".len()..].to_string();
        let mut event: serde_json::Value =
            serde_json::from_slice(&BASE64_STANDARD.decode(token).unwrap()).unwrap();
        event["pubkey"] = Keys::generate().public_key().to_hex().into();
        let mut forged = HeaderMap::new();
        let token = BASE64_STANDARD.encode(serde_json::to_vec(&event).unwrap());
        forged.insert(AUTHORIZATION, format!("Nostr {token}").parse().unwrap());

        let status = send(&store, Method::PUT, &forged, &body, fixture.now).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(store.tokens().is_empty());
    }

    #[tokio::test]
    async fn push_device_repeated_put_replaces_token_per_platform() {
        let store = MemoryDeviceStore::default();
        let keys = Keys::generate();
        for (platform, token) in [("android", "a1"), ("android", "a2"), ("ios", "i1")] {
            let body = body(platform, token);
            let headers = signed(&keys, "PUT", "https://relay.example.com/push", &body, NOW);
            assert_eq!(
                send(&store, Method::PUT, &headers, &body, NOW).await,
                StatusCode::NO_CONTENT
            );
        }
        let mut tokens = store.tokens();
        tokens.sort_by_key(|(_, platform, _)| platform.as_str());
        assert_eq!(
            tokens,
            vec![
                (keys.public_key(), Platform::Android, "a2".to_string()),
                (keys.public_key(), Platform::Ios, "i1".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn push_device_delete_of_unknown_token_keeps_registration() {
        let store = MemoryDeviceStore::default();
        let keys = Keys::generate();
        let registered = body("android", "current");
        let headers = signed(
            &keys,
            "PUT",
            "https://relay.example.com/push",
            &registered,
            NOW,
        );
        send(&store, Method::PUT, &headers, &registered, NOW).await;

        let stale = body("android", "stale");
        let headers = signed(
            &keys,
            "DELETE",
            "https://relay.example.com/push",
            &stale,
            NOW,
        );
        assert_eq!(
            send(&store, Method::DELETE, &headers, &stale, NOW).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            store.tokens(),
            vec![(keys.public_key(), Platform::Android, "current".to_string())]
        );
    }

    #[tokio::test]
    async fn push_device_rejects_malformed_body() {
        let store = MemoryDeviceStore::default();
        let keys = Keys::generate();
        let too_long = "t".repeat(MAX_TOKEN_LEN + 1);
        let bodies = [
            br#"{"platform":"windows","token":"t"}"#.to_vec(),
            br#"{"platform":"android","token":""}"#.to_vec(),
            br#"{"platform":"android"}"#.to_vec(),
            body("android", &too_long),
        ];
        for body in bodies {
            let headers = signed(&keys, "PUT", "https://relay.example.com/push", &body, NOW);
            assert_eq!(
                send(&store, Method::PUT, &headers, &body, NOW).await,
                StatusCode::BAD_REQUEST
            );
        }
        assert!(store.tokens().is_empty());
    }
}
