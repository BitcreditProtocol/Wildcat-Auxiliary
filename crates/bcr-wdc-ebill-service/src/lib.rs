// ----- standard library imports
use std::sync::Arc;
// ----- extra library imports
use axum::{
    Router,
    extract::FromRef,
    routing::{get, post, put},
};
use bcr_common::client::ebill::Client as EbillClient;
use bcr_ebill_api::{
    external::{
        bitcoin::BitcoinClient,
        court::CourtClient,
        email::EmailClient,
        file_storage::FileStorageClient,
        mint::{MintClient, MintClientApi},
    },
    service::{
        bill_service::{BillService, BillServiceApi},
        contact_service::{ContactService, ContactServiceApi},
        identity_service::{IdentityService, IdentityServiceApi},
        transport_service::TransportServiceApi,
    },
};
use bcr_ebill_core::protocol::{Address, City, Country, Email, Name, Zip};
use bcr_ebill_transport::{NostrClient, PushApi, PushService, create_transport_service};
// ----- local modules
mod convert;
mod error;
mod web;
// ----- end imports

#[derive(Clone, Debug, serde::Deserialize)]
pub struct AppConfig {
    pub ebill_db: ConnectionConfig,
    pub bitcoin_network: String,
    pub esplora_base_url: url::Url,
    pub nostr_cfg: NostrConfig,
    pub mint_config: MintConfig,
    pub payment_config: PaymentConfig,
    pub job_runner_initial_delay_seconds: u64,
    pub job_runner_check_interval_seconds: u64,
    pub url: url::Url,
    pub court_config: CourtConfig,
    pub dev_mode_config: DevModeConfig,
    pub identity_config: IdentityConfig,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct NostrConfig {
    pub only_known_contacts: bool,
    pub relays: Vec<url::Url>,
    pub blossom_servers: Vec<url::Url>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MintConfig {
    pub default_mint_url: url::Url,
    pub default_mint_node_id: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConnectionConfig {
    pub connection: String,
    pub namespace: String,
    pub database: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct PaymentConfig {
    pub num_confirmations_for_payment: usize,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CourtConfig {
    /// The default court URL
    pub default_url: url::Url,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DevModeConfig {
    /// Whether dev mode is on
    pub on: bool,
    /// Whether mandatory email confirmations should be enabled (disable for easier testing)
    pub mandatory_email_confirmations: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct IdentityConfig {
    pub name: Name,
    pub email: Email,
    pub country: Country,
    pub city: City,
    pub zip: Zip,
    pub address: Address,
}

#[derive(Clone, FromRef)]
pub struct AppController {
    pub contact_service: Arc<dyn ContactServiceApi>,
    pub bill_service: Arc<dyn BillServiceApi>,
    pub identity_service: Arc<dyn IdentityServiceApi>,
    pub notification_service: Arc<dyn TransportServiceApi>,
    pub push_service: Arc<dyn PushApi>,
    pub mint_client: Arc<dyn MintClientApi>,
}

impl AppController {
    pub async fn new(
        cfg: bcr_ebill_api::Config,
        nostr_client: Arc<NostrClient>,
        db: bcr_ebill_api::DbContext,
    ) -> Self {
        let push_service = Arc::new(PushService::new());
        let email_client = Arc::new(EmailClient::new());
        let mint_client = Arc::new(MintClient::new());

        let notification_service = create_transport_service(
            nostr_client,
            db.clone(),
            email_client.clone(),
            cfg.nostr_config.relays.to_owned(),
            push_service.clone(),
            mint_client.clone(),
        )
        .await
        .expect("Failed to create notification service");
        let file_upload_client = Arc::new(FileStorageClient::new());
        let contact_service = Arc::new(ContactService::new(
            db.contact_store.clone(),
            db.file_upload_store.clone(),
            file_upload_client.clone(),
            db.file_reference_store.clone(),
            db.identity_store.clone(),
            db.company_store.clone(),
            db.nostr_contact_store.clone(),
            notification_service.clone(),
            &cfg.clone(),
        ));

        let court_client = Arc::new(CourtClient::new());
        let bill_service = Arc::new(BillService::new(
            db.bill_store.clone(),
            db.bill_blockchain_store.clone(),
            db.identity_store.clone(),
            db.file_upload_store.clone(),
            file_upload_client.clone(),
            db.file_reference_store.clone(),
            Arc::new(BitcoinClient::new()),
            notification_service.clone(),
            db.identity_chain_store.clone(),
            db.company_chain_store.clone(),
            db.contact_store.clone(),
            db.company_store.clone(),
            db.mint_store.clone(),
            mint_client.clone(),
            court_client.clone(),
            db.nostr_contact_store.clone(),
        ));

        let identity_service = IdentityService::new(
            db.identity_store.clone(),
            db.file_upload_store.clone(),
            file_upload_client.clone(),
            db.file_reference_store.clone(),
            db.identity_chain_store.clone(),
            notification_service.clone(),
            email_client.clone(),
            db.email_notification_store.clone(),
        );

        Self {
            contact_service,
            bill_service,
            identity_service: Arc::new(identity_service),
            notification_service,
            push_service,
            mint_client,
        }
    }
}

pub fn routes(ctrl: AppController) -> Router {
    Router::new()
        .route(EbillClient::GET_IDENTITY_EP_V1, get(web::get_identity))
        .route(
            EbillClient::CREATE_IDENTITY_EP_V1,
            post(web::create_identity),
        )
        .route(
            EbillClient::BACKUP_SEED_PHRASE_EP_V1,
            get(web::get_seed_phrase),
        )
        .route(
            EbillClient::RESTORE_FROM_SEED_PHRASE_EP_V1,
            put(web::recover_from_seed_phrase),
        )
        .route(EbillClient::GET_BILLS_EP_V1, get(web::get_bills))
        .route(EbillClient::GET_BILL_EP_V1, get(web::get_bill_detail))
        .route(
            EbillClient::GET_BILL_PAYMENT_ACTIONS_EP_V1,
            get(web::get_bill_payment_actions),
        )
        .route(
            EbillClient::GET_BILL_HISTORY_EP_V1,
            get(web::get_bill_history),
        )
        .route(
            EbillClient::GET_BILL_PAYMENT_STATUS_EP_V1,
            get(web::get_bill_payment_status),
        )
        .route(
            EbillClient::GET_BILL_ENDORSEMENTS_EP_V1,
            get(web::get_bill_endorsements),
        )
        .route(
            EbillClient::GET_BILL_ATTACHMENT_EP_V1,
            get(web::get_bill_attachment),
        )
        .route(
            EbillClient::REQUEST_TO_PAY_BILL_EP_V1,
            put(web::request_to_pay_bill),
        )
        .route(
            EbillClient::PREPARE_REQUEST_TO_PAY_BILL_EP_V1,
            post(web::prepare_request_to_pay_bill),
        )
        .route(
            EbillClient::SYNC_BILL_CHAIN_EP_V1,
            post(web::sync_bill_chain),
        )
        .route(
            EbillClient::VALIDATE_AND_DECRYPT_SHARED_BILL_EP_V1,
            post(web::validate_and_decrypt_shared_bill),
        )
        .route(
            EbillClient::VALIDATE_ENDORSED_BILL_MATCHES_SHARED_BILL_EP_V1,
            post(web::validate_endorsed_bill_matches_shared_bill),
        )
        .route(
            EbillClient::GET_SHARED_BILL_HISTORY_EP_V1,
            post(web::get_shared_bill_history),
        )
        .route(
            EbillClient::GET_FILE_FROM_REQUEST_TO_MINT_EP_V1,
            get(web::get_encrypted_bill_file_from_request_to_mint),
        )
        .with_state(ctrl)
}
