pub mod file_store;

use std::io::Write;

use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use nostr::{
    hashes::{
        Hash,
        sha256::{self, Hash as Sha256Hash},
    },
    types::Url,
};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use tracing::{error, info};

use crate::AppState;

/// Request body for the /mirror endpoint
///
/// # Example
/// ```json
/// {"url": "https://example.com/blob.bin"}
/// ```
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MirrorRequest {
    pub url: Url,
}

/// Validates that a source URL is safe for server-side requests.
/// Returns an error message if the URL is not allowed.
fn is_disallowed_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => ipv4.is_loopback() || ipv4.is_private() || ipv4.is_link_local(),
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback() || ipv6.is_unique_local() || ipv6.is_unicast_link_local()
        }
    }
}

async fn validate_source_url(url: &Url) -> Option<&'static str> {
    // HTTPS-only check
    if url.scheme() != "https" {
        return Some("Only HTTPS URLs are allowed");
    }

    let host = match url.host_str() {
        Some(h) => h,
        None => return Some("Invalid URL: missing host"),
    };

    if host == "localhost" {
        return Some("Invalid IP address");
    }

    if let Ok(ip) = host.parse::<IpAddr>()
        && is_disallowed_ip(ip)
    {
        return Some("Invalid IP address");
    }

    let port = url.port_or_known_default().unwrap_or(443);
    if let Ok(addrs) = tokio::net::lookup_host((host, port)).await
        && addrs.into_iter().any(|addr| is_disallowed_ip(addr.ip()))
    {
        return Some("Invalid IP address");
    }

    None
}

/// Helper to create an error response with X-Reason header
fn mirror_error_response(status: StatusCode, reason: &str) -> Response {
    Response::builder()
        .status(status)
        .header("X-Reason", reason)
        .body(axum::body::Body::from(reason.to_string()))
        .unwrap()
}

const ENCRYPTION_PUB_KEY_BYTE_LEN: usize = 65; // we use uncompressed keys

/// For now, the only parts of the API we implement are
/// GET /<sha256> - get a file
/// PUT /upload - upload a file
///
/// Both endpoints work without Authorization, since all uploaded content is supposed to be encrypted
/// by the uploader (but potentially for someone else to decrypt).

#[derive(Debug, Clone, Serialize)]
pub struct BlobDescriptor {
    sha256: Sha256Hash,
    url: Url,
    size: usize,
    uploaded: i64,
}

#[derive(Debug, Clone)]
pub struct File {
    pub hash: Sha256Hash,
    pub bytes: Vec<u8>,
    pub size: i32,
}

impl BlobDescriptor {
    pub fn new(base_url: Url, hash: Sha256Hash, size: usize) -> Result<Self, anyhow::Error> {
        Ok(Self {
            sha256: hash,
            size,
            url: base_url.join(&hash.to_string())?,
            uploaded: chrono::Utc::now().timestamp(),
        })
    }
}

/// Checks the file size, hashes the file and stores it in the database, returning a
/// blob descriptor.
/// If the file already exists - simply returns the descriptor
pub async fn handle_upload(State(state): State<AppState>, body: Bytes) -> impl IntoResponse {
    let size = body.len();

    info!("Upload File called for {} bytes", size);
    // check size
    if size > state.cfg.max_file_size_bytes {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("File too big - max {} bytes", state.cfg.max_file_size_bytes),
        )
            .into_response();
    }

    if size == 0 {
        return (StatusCode::BAD_REQUEST, "Empty body").into_response();
    }
    // validate it's an ECIES/secp256k1 encrypted blob by checking if it starts with an ephemeral secp256k1 pub key
    // this is not a 100% guarantee (which is impossible), but rather a pretty reliable heuristic
    if size < ENCRYPTION_PUB_KEY_BYTE_LEN {
        error!("Non-encrypted Upload rejected - not big enough");
        return (StatusCode::BAD_REQUEST, "Invalid body").into_response();
    }
    let pubkey_bytes = &body[0..ENCRYPTION_PUB_KEY_BYTE_LEN];
    if let Err(e) = nostr::secp256k1::PublicKey::from_slice(pubkey_bytes) {
        error!("Non-encrypted Upload rejected: {e}");
        return (StatusCode::BAD_REQUEST, "Invalid body").into_response();
    }

    // create hash
    let mut hash_engine = sha256::HashEngine::default();
    if let Err(e) = hash_engine.write_all(&body) {
        error!("Error while hashing {size} bytes: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_SERVER_ERROR").into_response();
    }
    let hash = sha256::Hash::from_engine(hash_engine);

    let file = File {
        hash,
        bytes: body.into(),
        size: size as i32,
    };

    // store
    if let Err(e) = state.file_store.insert(file).await {
        error!("Error while storing {size} bytes with hash {hash}: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_SERVER_ERROR").into_response();
    }

    // return blob descriptor
    let blob_desc = BlobDescriptor::new(state.cfg.host_url, hash, size).unwrap();
    (StatusCode::OK, Json(blob_desc)).into_response()
}

/// Checks if there is a file with the given hash and returns it as application/octet-stream
/// since all our files are encrypted
pub async fn handle_get_file(
    State(state): State<AppState>,
    Path(hash): Path<Sha256Hash>,
) -> impl IntoResponse {
    info!("Get File called with hash {hash}");

    let file = match state.file_store.get(&hash).await {
        Ok(Some(file)) => file,
        Ok(None) => {
            error!("No file found with hash {hash}");
            return (StatusCode::NOT_FOUND, "NOT_FOUND").into_response();
        }
        Err(e) => {
            error!("Error while fetching file with hash {hash}: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_SERVER_ERROR").into_response();
        }
    };

    match Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/octet-stream")
        .body(Body::from(file.bytes))
    {
        Ok(resp) => resp,
        Err(e) => {
            error!("Error while creating response for {hash}: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_SERVER_ERROR").into_response()
        }
    }
}

pub async fn handle_list(Path(_pub_key): Path<String>) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "NOT_IMPLEMENTED")
}

/// Handles the /mirror endpoint by fetching a blob from a remote source,
/// comparing its SHA-256 hash against the caller-provided `x` tag, and storing it.
/// Returns 400 for malformed request metadata or hash mismatch, 401 for expired tokens,
/// 404 if the source is not accessible, 413 if the source is too large, and 500 for
/// internal errors.
pub async fn handle_mirror(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<MirrorRequest>,
) -> impl IntoResponse {
    info!("Mirror request for source: {}", request.url);

    // Validate source URL for SSRF protection
    if let Some(reason) = validate_source_url(&request.url).await {
        error!("Source URL validation failed: {}", reason);
        return mirror_error_response(StatusCode::BAD_REQUEST, reason);
    }

    let auth_token = match crate::auth::extract_auth_token(&headers) {
        Ok(token) => token,
        Err(e) => {
            error!("Auth token extraction failed: {:?}", e);
            return e.into_response();
        }
    };

    let expected_hash = match auth_token.require_x_tag() {
        Ok(hash) => hash,
        Err(e) => {
            error!("Missing x tag in auth token");
            return e.into_response();
        }
    };

    // Check if blob already exists locally before downloading
    match state.file_store.get(&expected_hash).await {
        Ok(Some(file)) => {
            info!(
                "Blob with hash {} already exists, returning existing descriptor",
                expected_hash
            );
            let blob_desc =
                match BlobDescriptor::new(state.cfg.host_url, expected_hash, file.size as usize) {
                    Ok(desc) => desc,
                    Err(e) => {
                        error!("Error creating blob descriptor: {}", e);
                        return mirror_error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Internal server error",
                        );
                    }
                };
            return (StatusCode::OK, Json(blob_desc)).into_response();
        }
        Ok(None) => {
            // Blob doesn't exist, continue with download
        }
        Err(e) => {
            error!(
                "Error checking for existing blob with hash {}: {}",
                expected_hash, e
            );
            return mirror_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error",
            );
        }
    }

    let response = match state.http_client.get(request.url.as_str()).send().await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Failed to fetch from source: {}", e);
            return mirror_error_response(StatusCode::NOT_FOUND, "Source blob not accessible");
        }
    };

    if !response.status().is_success() {
        error!("Source returned status: {}", response.status());
        return mirror_error_response(StatusCode::NOT_FOUND, "Source blob not accessible");
    }

    if let Some(len) = response.content_length()
        && len > state.cfg.max_file_size_bytes as u64
    {
        return mirror_error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!(
                "Source blob too large - max {} bytes",
                state.cfg.max_file_size_bytes
            ),
        );
    };

    let mut stream = response.bytes_stream();
    let mut total_size: usize = 0;
    let mut file_bytes = Vec::new();
    let mut hash_engine = sha256::HashEngine::default();

    while let Some(chunk_result) = stream.next().await {
        let chunk = match chunk_result {
            Ok(chunk) => chunk,
            Err(e) => {
                error!("Error while downloading chunk: {}", e);
                return mirror_error_response(StatusCode::NOT_FOUND, "Source blob not accessible");
            }
        };

        total_size += chunk.len();

        if total_size > state.cfg.max_file_size_bytes {
            return mirror_error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                &format!(
                    "Source blob too large - max {} bytes",
                    state.cfg.max_file_size_bytes
                ),
            );
        }

        if let Err(e) = hash_engine.write_all(&chunk) {
            error!("Error while hashing chunk: {}", e);
            return mirror_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error",
            );
        }

        file_bytes.extend_from_slice(&chunk);
    }

    let computed_hash = sha256::Hash::from_engine(hash_engine);

    if computed_hash != expected_hash {
        error!(
            "Hash mismatch: expected {} but computed {}",
            expected_hash, computed_hash
        );
        return mirror_error_response(
            StatusCode::BAD_REQUEST,
            "Hash mismatch - blob does not match expected SHA-256",
        );
    }

    if total_size < ENCRYPTION_PUB_KEY_BYTE_LEN {
        error!("Non-encrypted blob rejected - not big enough");
        return mirror_error_response(StatusCode::BAD_REQUEST, "Invalid blob format");
    }

    let pubkey_bytes = &file_bytes[0..ENCRYPTION_PUB_KEY_BYTE_LEN];
    if let Err(e) = nostr::secp256k1::PublicKey::from_slice(pubkey_bytes) {
        error!("Non-encrypted blob rejected: {}", e);
        return mirror_error_response(StatusCode::BAD_REQUEST, "Invalid blob format");
    }

    let file = File {
        hash: computed_hash,
        bytes: file_bytes,
        size: total_size as i32,
    };

    if let Err(e) = state.file_store.insert(file).await {
        error!(
            "Error while storing blob with hash {}: {}",
            computed_hash, e
        );
        return mirror_error_response(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error");
    }

    let blob_desc = match BlobDescriptor::new(state.cfg.host_url, computed_hash, total_size) {
        Ok(desc) => desc,
        Err(e) => {
            error!("Error creating blob descriptor: {}", e);
            return mirror_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error",
            );
        }
    };

    info!("Successfully mirrored blob with hash {}", computed_hash);
    (StatusCode::OK, Json(blob_desc)).into_response()
}

pub async fn handle_media() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "NOT_IMPLEMENTED")
}

pub async fn handle_report() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "NOT_IMPLEMENTED")
}

pub async fn handle_delete(Path(_hash): Path<String>) -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "NOT_IMPLEMENTED")
}

pub async fn handle_upload_head(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let content_length = match headers.get("X-Content-Length") {
        Some(val) => val,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                [("X-Reason", "Missing X-Content-Length header")],
                "",
            )
                .into_response();
        }
    };

    let size: usize = match content_length.to_str() {
        Ok(val) => match val.parse() {
            Ok(n) => n,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    [("X-Reason", "Invalid X-Content-Length header")],
                    "",
                )
                    .into_response();
            }
        },
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                [("X-Reason", "Invalid X-Content-Length header")],
                "",
            )
                .into_response();
        }
    };

    if size > state.cfg.max_file_size_bytes {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            [(
                "X-Reason",
                format!("File too big - max {} bytes", state.cfg.max_file_size_bytes),
            )],
            "",
        )
            .into_response();
    }

    (StatusCode::OK, "").into_response()
}

pub async fn handle_get_file_head(
    State(state): State<AppState>,
    Path(hash): Path<Sha256Hash>,
) -> impl IntoResponse {
    info!("HEAD File called with hash {hash}");

    let file = match state.file_store.get(&hash).await {
        Ok(Some(file)) => file,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Body::empty()).into_response();
        }
        Err(e) => {
            error!("Error while fetching file with hash {hash}: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Body::empty()).into_response();
        }
    };

    match Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/octet-stream")
        .header("Content-Length", file.size.to_string())
        .body(Body::empty())
    {
        Ok(resp) => resp,
        Err(e) => {
            error!("Error while creating HEAD response for {hash}: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Body::empty()).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::body::to_bytes;
    use nostr::{Kind, event::EventBuilder, key::Keys};
    use std::sync::{Arc, Mutex};

    /// Mock implementation of FileStoreApi for testing
    #[derive(Debug, Clone)]
    struct MockFileStore {
        files: Arc<Mutex<std::collections::HashMap<Sha256Hash, File>>>,
    }

    impl MockFileStore {
        fn new() -> Self {
            Self {
                files: Arc::new(Mutex::new(std::collections::HashMap::new())),
            }
        }

        #[allow(dead_code)]
        fn insert_sync(&self, file: File) {
            self.files.lock().unwrap().insert(file.hash, file);
        }

        #[allow(dead_code)]
        fn clear(&self) {
            self.files.lock().unwrap().clear();
        }
    }

    #[async_trait]
    impl file_store::FileStoreApi for MockFileStore {
        async fn get(&self, hash: &Sha256Hash) -> Result<Option<File>, anyhow::Error> {
            Ok(self.files.lock().unwrap().get(hash).cloned())
        }

        async fn insert(&self, file: File) -> Result<(), anyhow::Error> {
            self.files.lock().unwrap().insert(file.hash, file);
            Ok(())
        }
    }

    /// Test helper to create a valid encrypted blob (starts with valid secp256k1 pubkey)
    fn create_test_encrypted_blob(size: usize) -> Vec<u8> {
        let mut blob = vec![0u8; size.max(65)];
        blob[0] = 0x04;
        let valid_pubkey = [
            0x04u8, 0x79, 0xBE, 0x66, 0x7E, 0xF9, 0xDC, 0xBB, 0xAC, 0x55, 0xA0, 0x62, 0x95, 0xCE,
            0x87, 0x0B, 0x07, 0x02, 0x9B, 0xFC, 0xDB, 0x2D, 0xCE, 0x28, 0xD9, 0x59, 0xF2, 0x81,
            0x5B, 0x16, 0xF8, 0x17, 0x98, 0x48, 0x3A, 0xDA, 0x77, 0x26, 0xA3, 0xC4, 0x65, 0x5D,
            0xA4, 0xFB, 0xFC, 0x0E, 0x11, 0x08, 0xA8, 0xFD, 0x17, 0xB4, 0x48, 0xA6, 0x85, 0x54,
            0x19, 0x9C, 0x47, 0xD0, 0x8F, 0xFB, 0x10, 0xD4, 0xB8,
        ];
        blob[..65].copy_from_slice(&valid_pubkey);
        blob
    }

    fn create_test_state() -> AppState {
        let mock_store = Arc::new(MockFileStore::new());
        AppState {
            relay: nostr_relay_builder::LocalRelay::new(
                nostr_relay_builder::RelayBuilder::default(),
            ),
            cfg: crate::AppConfig {
                host_url: Url::parse("http://localhost:8080").unwrap(),
                max_file_size_bytes: 10_000_000, // 10MB
            },
            file_store: mock_store,
            http_client: Arc::new(reqwest::Client::new()),
        }
    }

    #[tokio::test]
    async fn test_head_file_exists() {
        let state = create_test_state();
        let blob = create_test_encrypted_blob(100);
        let mut hash_engine = sha256::HashEngine::default();
        hash_engine.write_all(&blob).unwrap();
        let hash = sha256::Hash::from_engine(hash_engine);

        // Insert the file
        let file = File {
            hash,
            bytes: blob.clone(),
            size: blob.len() as i32,
        };
        state.file_store.insert(file).await.unwrap();

        // Make the HEAD request
        let response = handle_get_file_head(State(state.clone()), Path(hash)).await;

        // Verify response
        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("Content-Type")
                .unwrap()
                .to_str()
                .unwrap(),
            "application/octet-stream"
        );
        assert_eq!(
            response
                .headers()
                .get("Content-Length")
                .unwrap()
                .to_str()
                .unwrap(),
            blob.len().to_string()
        );
    }

    #[tokio::test]
    async fn test_head_file_not_found() {
        let state = create_test_state();
        let hash = Sha256Hash::all_zeros();

        // Make the HEAD request without inserting any file
        let response = handle_get_file_head(State(state), Path(hash)).await;

        // Verify response is 404
        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_head_upload_valid_preflight() {
        let state = create_test_state();
        let mut headers = HeaderMap::new();
        headers.insert("X-Content-Length", "1024".parse().unwrap());

        let response = handle_upload_head(State(state), headers).await;

        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_head_upload_oversized() {
        let state = create_test_state();
        let mut headers = HeaderMap::new();
        // Exceed max file size
        headers.insert(
            "X-Content-Length",
            (state.cfg.max_file_size_bytes + 1)
                .to_string()
                .parse()
                .unwrap(),
        );

        let response = handle_upload_head(State(state), headers).await;

        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(response.headers().get("X-Reason").is_some());
    }

    #[tokio::test]
    async fn test_head_upload_missing_content_length() {
        let state = create_test_state();
        let headers = HeaderMap::new(); // No X-Content-Length header

        let response = handle_upload_head(State(state), headers).await;

        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get("X-Reason").unwrap(),
            "Missing X-Content-Length header"
        );
    }

    #[tokio::test]
    async fn test_head_upload_invalid_content_length() {
        let state = create_test_state();
        let mut headers = HeaderMap::new();
        headers.insert("X-Content-Length", "not-a-number".parse().unwrap());

        let response = handle_upload_head(State(state), headers).await;

        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get("X-Reason").unwrap(),
            "Invalid X-Content-Length header"
        );
    }

    fn create_auth_token(keys: &Keys, hash: &Sha256Hash, kind: u16, t_tag: &str) -> String {
        let event = EventBuilder::new(Kind::from(kind), "mirror request")
            .tag(nostr::Tag::parse(["t", t_tag]).unwrap())
            .tag(nostr::Tag::parse(["x", &hash.to_string()]).unwrap())
            .sign_with_keys(keys)
            .unwrap();

        let event_json = serde_json::to_string(&event).unwrap();
        let base64_token = base64_url::encode(&event_json);
        format!("Nostr {}", base64_token)
    }

    #[tokio::test]
    async fn test_mirror_source_url_validation_rejects_http() {
        let state = create_test_state();
        let keys = Keys::generate();
        let hash = Sha256Hash::all_zeros();
        let auth_header = create_auth_token(&keys, &hash, 24242, "upload");

        let mut headers = HeaderMap::new();
        headers.insert("Authorization", auth_header.parse().unwrap());

        // Try with HTTP (not HTTPS)
        let request = MirrorRequest {
            url: Url::parse("http://example.com/file.bin").unwrap(),
        };

        let response = handle_mirror(State(state), headers, Json(request)).await;

        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1000).await.unwrap();
        assert!(
            body.to_vec()
                .windows("Only HTTPS URLs are allowed".len())
                .any(|w| w == "Only HTTPS URLs are allowed".as_bytes())
        );
    }

    #[tokio::test]
    async fn test_mirror_source_url_validation_rejects_localhost() {
        let state = create_test_state();
        let keys = Keys::generate();
        let hash = Sha256Hash::all_zeros();
        let auth_header = create_auth_token(&keys, &hash, 24242, "upload");

        let mut headers = HeaderMap::new();
        headers.insert("Authorization", auth_header.parse().unwrap());

        // Try with localhost
        let request = MirrorRequest {
            url: Url::parse("https://localhost/file.bin").unwrap(),
        };

        let response = handle_mirror(State(state), headers, Json(request)).await;

        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_mirror_source_url_validation_rejects_private_ip() {
        let state = create_test_state();
        let keys = Keys::generate();
        let _hash = Sha256Hash::all_zeros();
        let auth_header = create_auth_token(&keys, &_hash, 24242, "upload");

        let mut headers = HeaderMap::new();
        headers.insert("Authorization", auth_header.parse().unwrap());

        // Try with private IP
        let request = MirrorRequest {
            url: Url::parse("https://192.168.1.1/file.bin").unwrap(),
        };

        let response = handle_mirror(State(state), headers, Json(request)).await;

        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_is_disallowed_ip_rejects_private_loopback_and_link_local() {
        assert!(is_disallowed_ip(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)));
        assert!(is_disallowed_ip(IpAddr::V4(std::net::Ipv4Addr::new(
            192, 168, 1, 10,
        ))));
        assert!(is_disallowed_ip(IpAddr::V4(std::net::Ipv4Addr::new(
            169, 254, 1, 10,
        ))));
        assert!(is_disallowed_ip(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)));
        assert!(is_disallowed_ip(IpAddr::V6(std::net::Ipv6Addr::new(
            0xfc00, 0, 0, 0, 0, 0, 0, 1,
        ))));
        assert!(is_disallowed_ip(IpAddr::V6(std::net::Ipv6Addr::new(
            0xfe80, 0, 0, 0, 0, 0, 0, 1,
        ))));
    }

    #[test]
    fn test_is_disallowed_ip_allows_public_addresses() {
        assert!(!is_disallowed_ip(IpAddr::V4(std::net::Ipv4Addr::new(
            88, 99, 122, 172,
        ))));
        assert!(!is_disallowed_ip(IpAddr::V6(std::net::Ipv6Addr::new(
            0x2606, 0x4700, 0, 0, 0, 0, 0, 0x1111,
        ))));
    }

    #[tokio::test]
    async fn test_mirror_missing_auth_header() {
        let state = create_test_state();
        let _hash = Sha256Hash::all_zeros();

        // No Authorization header
        let headers = HeaderMap::new();

        let request = MirrorRequest {
            url: Url::parse("https://example.com/file.bin").unwrap(),
        };

        let response = handle_mirror(State(state), headers, Json(request)).await;

        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_mirror_invalid_auth_format() {
        let state = create_test_state();
        let _hash = Sha256Hash::all_zeros();

        let mut headers = HeaderMap::new();
        // Wrong format (Bearer instead of Nostr)
        headers.insert("Authorization", "Bearer invalid_token".parse().unwrap());

        let request = MirrorRequest {
            url: Url::parse("https://example.com/file.bin").unwrap(),
        };

        let response = handle_mirror(State(state), headers, Json(request)).await;

        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_mirror_missing_x_tag() {
        let state = create_test_state();
        let keys = Keys::generate();

        // Create token without x tag
        let event = EventBuilder::new(Kind::from(24242), "mirror request")
            .tag(nostr::Tag::parse(["t", "upload"]).unwrap())
            .sign_with_keys(&keys)
            .unwrap();

        let event_json = serde_json::to_string(&event).unwrap();
        let base64_token = base64_url::encode(&event_json);
        let auth_header = format!("Nostr {}", base64_token);

        let mut headers = HeaderMap::new();
        headers.insert("Authorization", auth_header.parse().unwrap());

        let request = MirrorRequest {
            url: Url::parse("https://example.com/file.bin").unwrap(),
        };

        let response = handle_mirror(State(state), headers, Json(request)).await;

        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
