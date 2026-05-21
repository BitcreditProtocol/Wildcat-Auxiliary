use bcr_wdc_demo_faucet::AppConfig;
use std::{env, str::FromStr};
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_subscriber::{filter::LevelFilter, prelude::*};

#[derive(Debug, serde::Deserialize)]
struct MainConfig {
    log_level: String,
    appcfg: bcr_wdc_demo_faucet::AppConfig,
}

#[tokio::main]
async fn main() {
    let cfg_path =
        env::var("DEMO_FAUCET_CONFIG_FILE").unwrap_or_else(|_| "config.toml".to_string());
    let settings = config::Config::builder()
        .add_source(config::File::with_name(&cfg_path))
        .add_source(config::Environment::with_prefix("DEMO_FAUCET").separator("__"))
        .build()
        .expect("Failed to build demo faucet config");

    let maincfg: MainConfig = settings
        .try_deserialize()
        .expect("Failed to parse faucet demo config");

    tracing_log::LogTracer::init().expect("LogTracer init");
    let level_filter = LevelFilter::from_str(&maincfg.log_level).expect("log level");
    let stdout_log = tracing_subscriber::fmt::layer().with_filter(level_filter);
    let subscriber = tracing_subscriber::registry().with(stdout_log);
    tracing::subscriber::set_global_default(subscriber)
        .expect("tracing::subscriber::set_global_default");

    let app_cfg = AppConfig {
        quotes_url: maincfg.appcfg.quotes_url.clone(),
        sleep_secs: maincfg.appcfg.sleep_secs,
        retention_period_secs: maincfg.appcfg.retention_period_secs,
        max_requests_per_retention_period: maincfg.appcfg.max_requests_per_retention_period,
        max_bill_offer_sum: maincfg.appcfg.max_bill_offer_sum,
        discount_percent: maincfg.appcfg.discount_percent,
    };

    info!("Demo Faucet Service running with config: {:?}", &maincfg);
    let shutdown = CancellationToken::new();
    {
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            tracing::info!("Received shutdown event - shutting down");
            shutdown.cancel();
        });
    }

    loop {
        if shutdown.is_cancelled() {
            break;
        }

        let cancel_token = CancellationToken::new();
        let main_loop = bcr_wdc_demo_faucet::main_loop(app_cfg.clone(), cancel_token.clone());
        tokio::pin!(main_loop);

        let retry = tokio::select! {
            biased;
            _ = shutdown.cancelled() => {
                cancel_token.cancel();
                false
            }
            result = &mut main_loop => {
                match result {
                    Ok(_) => {
                        tracing::info!("Main loop completed successfully.");
                        false
                    },
                    Err(e) => {
                        tracing::error!("Main loop encountered error: {e}");
                        true
                    },
                }
            }
        };

        if !retry {
            break;
        }
        tracing::info!("restarting loop in 1 second..");
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
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
