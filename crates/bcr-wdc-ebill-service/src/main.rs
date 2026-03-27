// ----- standard library imports
use bcr_common::core::NodeId;
use bcr_ebill_api::{CourtConfig, DevModeConfig, MintConfig, NostrConfig, PaymentConfig};
use bcr_ebill_core::protocol::{
    OptionalPostalAddress, Timestamp, blockchain::identity::IdentityType, crypto::BcrKeys,
};
use bcr_ebill_transport::{
    chain_keys::ChainKeyService, create_nostr_clients, create_nostr_consumer,
};
use std::{env, str::FromStr};
// ----- extra library imports
use tokio::signal;
use tracing::info;
use tracing_subscriber::{
    filter::{LevelFilter, Targets},
    prelude::*,
};
// ----- local modules
mod job;
// ----- end imports

#[derive(Debug, serde::Deserialize)]
struct MainConfig {
    bind_address: std::net::SocketAddr,
    appcfg: bcr_wdc_ebill_service::AppConfig,
    log_level: String,
    restart_nostr_consumer_interval_secs: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
struct SeedConfig {
    mnemonic: bip39::Mnemonic,
}

#[tokio::main]
async fn main() {
    let cfg_path = env::var("EBILL_CONFIG_FILE").unwrap_or_else(|_| "config.toml".to_string());
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install default provider for rustls ring");
    // parse and create config
    let settings = config::Config::builder()
        .add_source(config::File::with_name(&cfg_path))
        .add_source(
            config::Environment::with_prefix("EBILL")
                .separator("__")
                .list_separator(",")
                .with_list_parse_key("appcfg.nostr_cfg.relays")
                .with_list_parse_key("appcfg.nostr_cfg.blossom_servers")
                .try_parsing(true),
        )
        .build()
        .expect("Failed to build ebill config");

    let maincfg: MainConfig = settings
        .try_deserialize()
        .expect("Failed to parse ebill config");

    // seed is acquired from environment variables
    let settings = config::Config::builder()
        .add_source(config::Environment::with_prefix("EBILL"))
        .build()
        .expect("Failed to build seed config");

    let seedcfg: SeedConfig = settings
        .try_deserialize()
        .expect("Failed to parse seed config");
    let keys_from_mnemonic = BcrKeys::from_seedphrase(&seedcfg.mnemonic.to_string())
        .expect("Failed to build keys from seed phrase");

    tracing_log::LogTracer::init().expect("LogTracer init");
    let level_filter = LevelFilter::from_str(&maincfg.log_level).expect("log level");
    let targets = Targets::new()
        .with_default(LevelFilter::INFO)
        .with_target("nostr_relay_pool", LevelFilter::INFO)
        .with_target("bcr_ebill_api", level_filter)
        .with_target("bcr_ebill_core", level_filter)
        .with_target("bcr_ebill_transport", level_filter)
        .with_target("bcr_ebill_persistence", level_filter)
        .with_target("bcr_common", level_filter);
    let stdout_log = tracing_subscriber::fmt::layer()
        .with_filter(level_filter)
        .with_filter(targets);
    let subscriber = tracing_subscriber::registry().with(stdout_log);
    tracing::subscriber::set_global_default(subscriber)
        .expect("tracing::subscriber::set_global_default");

    // create bcr_ebill_api config
    let api_config = bcr_ebill_api::Config {
        court_config: CourtConfig {
            default_url: maincfg.appcfg.court_config.default_url.clone(),
        },
        dev_mode_config: DevModeConfig {
            on: maincfg.appcfg.dev_mode_config.on,
            disable_mandatory_email_confirmations: maincfg
                .appcfg
                .dev_mode_config
                .disable_mandatory_email_confirmations,
        },
        bitcoin_network: maincfg.appcfg.bitcoin_network.clone(),
        esplora_base_urls: vec![maincfg.appcfg.esplora_base_url.clone()],
        nostr_config: NostrConfig {
            relays: maincfg.appcfg.nostr_cfg.relays.clone(),
            blossom_servers: maincfg.appcfg.nostr_cfg.blossom_servers.clone(),
            only_known_contacts: maincfg.appcfg.nostr_cfg.only_known_contacts,
            max_relays: Some(50),
        },
        mint_config: MintConfig {
            default_mint_url: maincfg.appcfg.mint_config.default_mint_url.clone(),
            default_mint_node_id: NodeId::from_str(
                &maincfg.appcfg.mint_config.default_mint_node_id,
            )
            .expect("Invalid Mint Node Id"),
        },
        db_config: bcr_ebill_persistence::db::SurrealDbConfig {
            connection_string: maincfg.appcfg.ebill_db.connection.clone(),
            namespace: maincfg.appcfg.ebill_db.namespace.clone(),
            database: maincfg.appcfg.ebill_db.database.clone(),
        },
        files_db_config: bcr_ebill_persistence::db::SurrealDbConfig {
            connection_string: maincfg.appcfg.ebill_db.connection.clone(),
            namespace: maincfg.appcfg.ebill_db.namespace.clone(),
            database: maincfg.appcfg.ebill_db.database.clone(),
        },
        app_url: maincfg.appcfg.url.clone(),
        payment_config: PaymentConfig {
            num_confirmations_for_payment: maincfg
                .appcfg
                .payment_config
                .num_confirmations_for_payment,
        },
    };
    bcr_ebill_api::init(api_config.clone()).expect("Could not initialize E-Bill API");

    // initialize DB context
    let db = bcr_ebill_api::get_db_context(&api_config)
        .await
        .expect("Failed to create DB context");
    // set the network and check if the configured network matches the persisted network and fail, if not
    db.identity_store
        .set_or_check_network(api_config.bitcoin_network())
        .await
        .expect("Couldn't set, or check btc network");

    // initialize identity keys from mnemonic, if they're not set
    let keys = match db.identity_store.get_key_pair().await {
        Ok(keys) => keys,
        Err(_) => {
            info!("No key pair found - setting it from given mnemonic");
            db.identity_store
                .save_key_pair(&keys_from_mnemonic, &seedcfg.mnemonic.to_string())
                .await
                .expect("Could not create key from mnemonic");
            keys_from_mnemonic.clone()
        }
    };

    if keys != keys_from_mnemonic {
        panic!("Keys from mnemonic don't match keys in the database");
    }

    let local_node_id = NodeId::new(keys.pub_key(), api_config.bitcoin_network());
    info!("Local node id: {local_node_id}");
    info!("Local npub as hex: {:?}", local_node_id.npub().to_hex());

    // set up nostr clients for existing identities
    let nostr_client = create_nostr_clients(
        &api_config,
        db.identity_store.clone(),
        db.company_store.clone(),
        db.nostr_contact_store.clone(),
    )
    .await
    .expect("Failed to create nostr clients");

    let db_clone = db.clone();
    // set up application context
    let app = bcr_wdc_ebill_service::AppController::new(api_config, nostr_client.clone(), db).await;

    // create identity if it doesn't exist
    if !app.identity_service.identity_exists().await {
        info!("No identity found - creating from config");
        app.identity_service
            .create_identity(
                IdentityType::Ident,
                maincfg.appcfg.identity_config.name.clone(),
                Some(maincfg.appcfg.identity_config.email.clone()),
                OptionalPostalAddress {
                    country: Some(maincfg.appcfg.identity_config.country.clone()),
                    city: Some(maincfg.appcfg.identity_config.city.clone()),
                    zip: Some(maincfg.appcfg.identity_config.zip.clone()),
                    address: Some(maincfg.appcfg.identity_config.address.clone()),
                },
                None,
                None,
                None,
                None,
                None,
                None,
                Timestamp::now(),
            )
            .await
            .expect("Failed to create identity");
    }

    let router = bcr_wdc_ebill_service::routes(app.clone());

    // run jobs in background
    let app_clone = app.clone();
    tokio::spawn(async move {
        job::run(
            app_clone.clone(),
            maincfg.appcfg.job_runner_initial_delay_seconds,
            maincfg.appcfg.job_runner_check_interval_seconds,
        )
        .await
    });

    // set up nostr event consumer
    let nostr_consumer = create_nostr_consumer(
        nostr_client.clone(),
        app.contact_service.clone(),
        app.push_service.clone(),
        std::sync::Arc::new(ChainKeyService::new(
            db_clone.bill_store.clone(),
            db_clone.company_store.clone(),
            db_clone.identity_store.clone(),
        )),
        db_clone.clone(),
    )
    .await
    .expect("Failed to create Nostr consumer");

    let nostr_consumer_restart_interval =
        maincfg.restart_nostr_consumer_interval_secs.unwrap_or(120);

    // run nostr consumer in the background and restart regularly so we don't drop events
    let nostr_handle = tokio::spawn(async move {
        info!(
            "Starting Nostr Consumer Loop - restart interval {}s",
            nostr_consumer_restart_interval
        );
        let interval = std::time::Duration::from_secs(nostr_consumer_restart_interval);

        loop {
            let mut joinset = nostr_consumer.start().await.expect("nostr consumer failed");

            // for each interval, or if a task exits, we re-start the consumer
            let reason = tokio::select! {
                _ = tokio::time::sleep(interval) => "interval elapsed",
                _ = joinset.join_next() => "a task finished",
            };

            info!("Restarting nostr consumer - reason: {reason}");

            joinset.abort_all();

            // flush tasks, so we don't leak tasks
            while joinset.join_next().await.is_some() {
                info!("Nostr consumer task shutdown");
            }

            // unsubscribe all, so we re-subscribe again on re-connect
            if let Ok(cl) = nostr_client.client().await {
                cl.unsubscribe_all().await;
            }
        }
    });

    // run web server
    let listener = tokio::net::TcpListener::bind(&maincfg.bind_address)
        .await
        .expect("Failed to bind to address");

    info!(
        "E-Bill Service running at http://{} with config: {:?}",
        &maincfg.bind_address, &maincfg
    );
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Failed to start server");
    nostr_handle.abort();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
