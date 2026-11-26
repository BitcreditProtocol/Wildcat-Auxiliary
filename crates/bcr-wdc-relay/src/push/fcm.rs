use super::{
    Device, Platform,
    device_store::DeviceStoreApi,
    queue::{PushPayload, PushSender},
};
use anyhow::{Context, anyhow};
use async_trait::async_trait;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tracing::{info, warn};

const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const JWT_GRANT: &str = "urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer";
const ACCESS_TOKEN_LIFETIME_SECS: u64 = 3600;
const ACCESS_TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(60);
const MAX_ATTEMPTS: u32 = 4;
const BASE_BACKOFF: Duration = Duration::from_millis(500);
const ALERT_TITLE: &str = "Bitcredit";
const ALERT_BODY: &str = "You have a new message";

/// Google service account credentials, as downloaded from the Firebase console
#[derive(Clone, Deserialize)]
pub struct ServiceAccount {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
    pub token_uri: String,
}

impl ServiceAccount {
    pub fn from_file(path: &Path) -> Result<Self, anyhow::Error> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading FCM credentials {}", path.display()))?;
        serde_json::from_str(&raw).context("parsing FCM credentials")
    }
}

/// Why one FCM request failed
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// Worth retrying: rate limits, server errors, network errors, an expired access token
    Transient(String),
    /// FCM says the registration token is dead
    InvalidToken(String),
    /// Any other rejection; retrying will not help
    Rejected(String),
}

/// Posts one FCM v1 message
#[async_trait]
pub trait FcmClient: Send + Sync {
    async fn post(&self, message: &Value) -> Result<(), SendError>;
}

/// The FCM v1 request body for `device`
pub fn build_message(payload: &PushPayload, device: &Device) -> Value {
    let data = json!({
        "event_id": payload.event_id.to_hex(),
        "relay_url": payload.relay_url,
        "kind": payload.kind.to_string(),
    });
    let message = match device.platform {
        Platform::Android => json!({
            "token": device.token,
            "data": data,
            "android": {"priority": "high"},
        }),
        Platform::Ios => json!({
            "token": device.token,
            "data": data,
            "notification": {"title": ALERT_TITLE, "body": ALERT_BODY},
            "apns": {
                "headers": {"apns-push-type": "alert", "apns-priority": "10"},
                "payload": {
                    "aps": {
                        "alert": {"title": ALERT_TITLE, "body": ALERT_BODY},
                        "mutable-content": 1,
                    }
                },
            },
        }),
    };
    json!({ "message": message })
}

/// Maps an FCM error response to a [`SendError`]
pub fn classify(status: u16, body: &str) -> SendError {
    let error = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").cloned())
        .unwrap_or(Value::Null);
    let status_name = error.get("status").and_then(Value::as_str).unwrap_or("");
    let message = error.get("message").and_then(Value::as_str).unwrap_or("");
    let details = error
        .get("details")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let summary = format!("{status} {status_name}: {message}");

    let fcm_code = details
        .iter()
        .find_map(|d| d.get("errorCode").and_then(Value::as_str))
        .unwrap_or("");
    if fcm_code == "UNREGISTERED" {
        return SendError::InvalidToken(summary);
    }
    if status_name == "INVALID_ARGUMENT" || fcm_code == "INVALID_ARGUMENT" {
        let names_token = message.contains("message.token")
            || message.to_lowercase().contains("registration token")
            || details.iter().any(|d| {
                d.get("fieldViolations")
                    .and_then(Value::as_array)
                    .is_some_and(|v| {
                        v.iter().any(|f| {
                            f.get("field").and_then(Value::as_str) == Some("message.token")
                        })
                    })
            });
        return if names_token {
            SendError::InvalidToken(summary)
        } else {
            SendError::Rejected(summary)
        };
    }
    if status == 401
        || status == 429
        || status >= 500
        || matches!(fcm_code, "UNAVAILABLE" | "INTERNAL" | "QUOTA_EXCEEDED")
    {
        return SendError::Transient(summary);
    }
    SendError::Rejected(summary)
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

#[derive(Deserialize)]
struct AccessTokenResponse {
    access_token: String,
    expires_in: u64,
}

/// Talks to the FCM v1 HTTP API, authenticating with a service account
pub struct HttpFcmClient {
    http: reqwest::Client,
    account: ServiceAccount,
    send_url: String,
    token: Mutex<Option<(String, Instant)>>,
}

impl HttpFcmClient {
    pub fn new(http: reqwest::Client, account: ServiceAccount, base_url: &str) -> Self {
        let send_url = format!(
            "{}/v1/projects/{}/messages:send",
            base_url.trim_end_matches('/'),
            account.project_id
        );
        Self {
            http,
            account,
            send_url,
            token: Mutex::new(None),
        }
    }

    async fn access_token(&self) -> Result<String, SendError> {
        let mut cached = self.token.lock().await;
        if let Some((token, expires)) = cached.as_ref()
            && Instant::now() + ACCESS_TOKEN_REFRESH_MARGIN < *expires
        {
            return Ok(token.clone());
        }
        let fresh = self.fetch_access_token().await?;
        *cached = Some((
            fresh.access_token.clone(),
            Instant::now() + Duration::from_secs(fresh.expires_in),
        ));
        Ok(fresh.access_token)
    }

    async fn fetch_access_token(&self) -> Result<AccessTokenResponse, SendError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| SendError::Rejected(e.to_string()))?
            .as_secs();
        let claims = Claims {
            iss: &self.account.client_email,
            scope: FCM_SCOPE,
            aud: &self.account.token_uri,
            iat: now,
            exp: now + ACCESS_TOKEN_LIFETIME_SECS,
        };
        let key = EncodingKey::from_rsa_pem(self.account.private_key.as_bytes())
            .map_err(|e| SendError::Rejected(format!("invalid FCM private key: {e}")))?;
        let assertion = jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &key)
            .map_err(|e| SendError::Rejected(format!("signing FCM assertion failed: {e}")))?;
        let response = self
            .http
            .post(&self.account.token_uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("grant_type={JWT_GRANT}&assertion={assertion}"))
            .send()
            .await
            .map_err(|e| SendError::Transient(format!("access token request failed: {e}")))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| SendError::Transient(format!("access token response failed: {e}")))?;
        if status.is_success() {
            return serde_json::from_str(&body)
                .map_err(|e| SendError::Rejected(format!("bad access token response: {e}")));
        }
        let detail = format!("access token request returned {status}");
        if status.as_u16() == 429 || status.is_server_error() {
            Err(SendError::Transient(detail))
        } else {
            Err(SendError::Rejected(detail))
        }
    }
}

#[async_trait]
impl FcmClient for HttpFcmClient {
    async fn post(&self, message: &Value) -> Result<(), SendError> {
        let token = self.access_token().await?;
        let response = self
            .http
            .post(&self.send_url)
            .bearer_auth(token)
            .header("content-type", "application/json")
            .body(message.to_string())
            .send()
            .await
            .map_err(|e| SendError::Transient(format!("FCM request failed: {e}")))?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        let error = classify(status.as_u16(), &body);
        if status.as_u16() == 401 {
            *self.token.lock().await = None;
        }
        Err(error)
    }
}

/// Sends pushes through FCM, retrying transient failures and dropping dead tokens
pub struct FcmSender {
    client: Arc<dyn FcmClient>,
    devices: Arc<dyn DeviceStoreApi>,
    backoff: Duration,
}

impl FcmSender {
    pub fn new(client: Arc<dyn FcmClient>, devices: Arc<dyn DeviceStoreApi>) -> Self {
        Self {
            client,
            devices,
            backoff: BASE_BACKOFF,
        }
    }

    async fn send_one(&self, payload: &PushPayload, device: &Device) -> Result<(), anyhow::Error> {
        let message = build_message(payload, device);
        let mut delay = self.backoff;
        for attempt in 1..=MAX_ATTEMPTS {
            match self.client.post(&message).await {
                Ok(()) => return Ok(()),
                Err(SendError::Transient(e)) if attempt < MAX_ATTEMPTS => {
                    warn!("FCM attempt {attempt} for {} failed: {e}", payload.event_id);
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
                Err(SendError::Transient(e) | SendError::Rejected(e)) => return Err(anyhow!(e)),
                Err(SendError::InvalidToken(e)) => {
                    info!(
                        "Removing {} token of {}: {e}",
                        device.platform.as_str(),
                        device.key
                    );
                    self.devices.remove(device).await?;
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl PushSender for FcmSender {
    async fn send(&self, payload: &PushPayload, devices: &[Device]) -> Result<(), anyhow::Error> {
        let mut failures = Vec::new();
        for device in devices {
            if let Err(e) = self.send_one(payload, device).await {
                failures.push(format!("{}: {e}", device.platform.as_str()));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(anyhow!(failures.join("; ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::device_store::MemoryDeviceStore;
    use nostr::{event::EventId, key::Keys};
    use std::{collections::VecDeque, sync::Mutex as StdMutex};

    struct ScriptedClient {
        results: StdMutex<VecDeque<Result<(), SendError>>>,
        posted: StdMutex<Vec<Value>>,
    }

    impl ScriptedClient {
        fn new(results: Vec<Result<(), SendError>>) -> Arc<Self> {
            Arc::new(Self {
                results: StdMutex::new(results.into()),
                posted: StdMutex::default(),
            })
        }
    }

    #[async_trait]
    impl FcmClient for ScriptedClient {
        async fn post(&self, message: &Value) -> Result<(), SendError> {
            self.posted.lock().unwrap().push(message.clone());
            self.results.lock().unwrap().pop_front().unwrap_or(Ok(()))
        }
    }

    fn test_id() -> EventId {
        EventId::from_hex(&"0".repeat(64)).unwrap()
    }

    fn payload() -> PushPayload {
        PushPayload {
            event_id: test_id(),
            relay_url: "wss://relay.example".to_string(),
            kind: 1059,
        }
    }

    fn device(platform: Platform) -> Device {
        Device {
            key: Keys::generate().public_key(),
            platform,
            token: "tok".to_string(),
        }
    }

    fn sender(client: Arc<ScriptedClient>, store: Arc<MemoryDeviceStore>) -> FcmSender {
        FcmSender {
            client,
            devices: store,
            backoff: Duration::ZERO,
        }
    }

    #[test]
    fn push_fcm_android_body_is_data_only_high_priority() {
        let body = build_message(&payload(), &device(Platform::Android));
        assert_eq!(
            body,
            json!({"message": {
                "token": "tok",
                "data": {
                    "event_id": test_id().to_hex(),
                    "relay_url": "wss://relay.example",
                    "kind": "1059",
                },
                "android": {"priority": "high"},
            }})
        );
    }

    #[test]
    fn push_fcm_ios_body_is_alert_with_mutable_content() {
        let body = build_message(&payload(), &device(Platform::Ios));
        let message = &body["message"];
        assert_eq!(message["apns"]["headers"]["apns-push-type"], "alert");
        assert_eq!(message["apns"]["headers"]["apns-priority"], "10");
        assert_eq!(message["apns"]["payload"]["aps"]["mutable-content"], 1);
        assert!(message["apns"]["payload"]["aps"]["alert"]["body"].is_string());
        assert_eq!(message["data"]["kind"], "1059");
        assert!(message.get("android").is_none());
    }

    #[test]
    fn push_fcm_404_without_unregistered_code() {
        let wrong_project = r#"{"error":{"code":404,"status":"NOT_FOUND","message":"Requested entity was not found."}}"#;
        for body in [wrong_project, "{}", ""] {
            assert!(matches!(classify(404, body), SendError::Rejected(_)));
        }
    }

    #[test]
    fn push_fcm_classifies_error_responses() {
        let unregistered = r#"{"error":{"code":404,"status":"NOT_FOUND","message":"gone","details":[{"@type":"type.googleapis.com/google.firebase.fcm.v1.FcmError","errorCode":"UNREGISTERED"}]}}"#;
        assert!(matches!(
            classify(404, unregistered),
            SendError::InvalidToken(_)
        ));
        let bad_token = r#"{"error":{"code":400,"status":"INVALID_ARGUMENT","message":"The registration token is not a valid FCM registration token"}}"#;
        assert!(matches!(
            classify(400, bad_token),
            SendError::InvalidToken(_)
        ));
        let field = r#"{"error":{"code":400,"status":"INVALID_ARGUMENT","message":"bad","details":[{"fieldViolations":[{"field":"message.token"}]}]}}"#;
        assert!(matches!(classify(400, field), SendError::InvalidToken(_)));
        let other = r#"{"error":{"code":400,"status":"INVALID_ARGUMENT","message":"Invalid value at message.data"}}"#;
        assert!(matches!(classify(400, other), SendError::Rejected(_)));
        assert!(matches!(classify(503, "{}"), SendError::Transient(_)));
        assert!(matches!(classify(429, "not json"), SendError::Transient(_)));
        assert!(matches!(classify(403, "{}"), SendError::Rejected(_)));
    }

    #[tokio::test]
    async fn push_fcm_retries_transient_errors() {
        let client = ScriptedClient::new(vec![
            Err(SendError::Transient("503".into())),
            Err(SendError::Transient("503".into())),
            Ok(()),
        ]);
        let sender = sender(client.clone(), Arc::new(MemoryDeviceStore::default()));
        sender
            .send(&payload(), &[device(Platform::Android)])
            .await
            .unwrap();
        assert_eq!(client.posted.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn push_fcm_gives_up_after_max_attempts() {
        let client = ScriptedClient::new(
            (0..10)
                .map(|_| Err(SendError::Transient("503".into())))
                .collect(),
        );
        let sender = sender(client.clone(), Arc::new(MemoryDeviceStore::default()));
        assert!(
            sender
                .send(&payload(), &[device(Platform::Android)])
                .await
                .is_err()
        );
        assert_eq!(client.posted.lock().unwrap().len(), MAX_ATTEMPTS as usize);
    }

    #[tokio::test]
    async fn push_fcm_removes_invalid_token_without_retry() {
        let store = Arc::new(MemoryDeviceStore::default());
        let dead = device(Platform::Ios);
        let live = device(Platform::Android);
        store.upsert(&dead).await.unwrap();
        store.upsert(&live).await.unwrap();
        let client = ScriptedClient::new(vec![Err(SendError::InvalidToken("x".into())), Ok(())]);
        let sender = sender(client.clone(), store.clone());
        sender
            .send(&payload(), &[dead.clone(), live.clone()])
            .await
            .unwrap();
        assert_eq!(client.posted.lock().unwrap().len(), 2);
        let left = store.tokens();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].1, Platform::Android);
    }

    #[tokio::test]
    async fn push_fcm_keeps_token_on_rejection() {
        let store = Arc::new(MemoryDeviceStore::default());
        let d = device(Platform::Android);
        store.upsert(&d).await.unwrap();
        let client = ScriptedClient::new(vec![Err(SendError::Rejected("403".into()))]);
        let sender = sender(client.clone(), store.clone());
        assert!(sender.send(&payload(), &[d]).await.is_err());
        assert_eq!(client.posted.lock().unwrap().len(), 1);
        assert_eq!(store.tokens().len(), 1);
    }
}
