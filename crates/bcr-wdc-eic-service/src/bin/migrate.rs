//! Copies the SurrealDB data that must survive into PostgreSQL.
//! Only `email_registrations` is copied: challenges and email confirmations are short-lived and start empty.
use anyhow::{Result, anyhow};
use bcr_wdc_eic_service::{
    AppConfig, MainConfig,
    persistence::{sql, surreal},
};
use bcr_wdc_shared::db::sql::{connect, run_migrations};
use std::env;
use std::str::FromStr;
use tracing::info;
use tracing_subscriber::{filter::LevelFilter, prelude::*};

#[tokio::main]
async fn main() {
    let dry_run = env::args().skip(1).any(|arg| arg == "--dry-run");

    // same configuration as the service
    let cfg_path = env::var("EIC_CONFIG_FILE").unwrap_or_else(|_| "config.toml".to_string());
    let settings = config::Config::builder()
        .add_source(config::File::with_name(&cfg_path))
        .add_source(config::Environment::with_prefix("EIC").separator("__"))
        .build()
        .expect("Failed to build eic config");

    let maincfg: MainConfig = settings
        .try_deserialize()
        .expect("Failed to parse eic config");

    tracing_log::LogTracer::init().expect("LogTracer init");
    let level_filter = LevelFilter::from_str(&maincfg.log_level).expect("log level");
    let stdout_log = tracing_subscriber::fmt::layer().with_filter(level_filter);
    let subscriber = tracing_subscriber::registry().with(stdout_log);
    tracing::subscriber::set_global_default(subscriber)
        .expect("tracing::subscriber::set_global_default");

    if let Err(e) = run(maincfg.appcfg, dry_run).await {
        tracing::error!("migration failed: {e:#}");
        std::process::exit(1);
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Summary {
    personal_registrations: usize,
    company_registrations: usize,
}

async fn run(cfg: AppConfig, dry_run: bool) -> Result<()> {
    if !dry_run && cfg.repository_new.max_connections == 0 {
        return Err(anyhow!(
            "PostgreSQL is not configured: set appcfg.repository_new (max_connections > 0)"
        ));
    }
    let surreal = surreal::DBEmails::new(cfg.email_confirmations)
        .await
        .map_err(|e| anyhow!(e))?;
    if surreal.is_migrated().await? {
        info!("SurrealDB is already migrated, nothing to do");
        return Ok(());
    }
    let target = if dry_run {
        None
    } else {
        let pool = connect(&cfg.repository_new).await?;
        run_migrations(&pool).await?;
        Some(sql::DB::from_pool(pool))
    };
    let summary = copy(&surreal, target.as_ref()).await?;
    info!(
        "email registrations: {} personal, {} company{}",
        summary.personal_registrations,
        summary.company_registrations,
        if dry_run {
            " (dry run, nothing copied)"
        } else {
            ""
        }
    );
    Ok(())
}

/// With no `target` only reads SurrealDB.
/// Otherwise copies into `target` and marks SurrealDB as migrated as the very last step.
async fn copy(surreal: &surreal::DBEmails, target: Option<&sql::DB>) -> Result<Summary> {
    let registrations = surreal.dump_email_registrations().await?;
    let company_registrations = registrations
        .iter()
        .filter(|r| r.company_node_id.is_some())
        .count();
    let summary = Summary {
        personal_registrations: registrations.len() - company_registrations,
        company_registrations,
    };
    let Some(target) = target else {
        return Ok(summary);
    };
    let inserted = target.import_email_registrations(&registrations).await?;
    if inserted != registrations.len() as u64 {
        return Err(anyhow!(
            "inserted {inserted} email registrations, expected {}",
            registrations.len()
        ));
    }
    surreal.mark_migrated().await?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bcr_common::core::NodeId;
    use bcr_wdc_eic_service::EmailConfirmationRepository;
    use bitcoin::{
        Network,
        secp256k1::{SECP256K1, SecretKey, schnorr::Signature},
    };
    use email_address::EmailAddress;

    async fn init_surreal_db() -> surreal::DBEmails {
        surreal::DBEmails::new(surreal::EmailConnectionConfig {
            connection: String::from("mem://"),
            namespace: String::from("test"),
            database: String::from("test"),
            table_confirmations: String::from("email_confirmations"),
            table_registrations: String::from("email_registrations"),
        })
        .await
        .expect("mem db")
    }

    fn random_node_id() -> NodeId {
        let sk = SecretKey::new(&mut bitcoin::secp256k1::rand::thread_rng());
        NodeId::new(sk.public_key(SECP256K1), Network::Testnet)
    }

    fn email(s: &str) -> EmailAddress {
        EmailAddress::from_str(s).unwrap()
    }

    fn random_signature() -> Signature {
        let sk = SecretKey::new(&mut bitcoin::secp256k1::rand::thread_rng());
        bcr_wdc_shared::signature::sign_payload(b"payload", &sk)
    }

    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_copy_email_registrations(pool: ::sqlx::PgPool) {
        let surreal = init_surreal_db().await;
        let target = sql::DB::from_pool(pool);
        let node_id = random_node_id();
        let company = Some(random_node_id());
        let personal_signature = random_signature();
        let company_signature = random_signature();
        surreal
            .insert_email_registration_for_node_id(
                &node_id,
                &None,
                &email("a@b.cr"),
                &personal_signature,
            )
            .await
            .unwrap();
        surreal
            .insert_email_registration_for_node_id(
                &node_id,
                &company,
                &email("c@d.cr"),
                &company_signature,
            )
            .await
            .unwrap();
        let expected = Summary {
            personal_registrations: 1,
            company_registrations: 1,
        };

        // dry run: only reads SurrealDB
        assert_eq!(copy(&surreal, None).await.unwrap(), expected);
        assert!(!surreal.is_migrated().await.unwrap());
        assert!(
            target
                .get_email_registration_for_node_id(&node_id, &None)
                .await
                .unwrap()
                .is_none()
        );

        assert_eq!(copy(&surreal, Some(&target)).await.unwrap(), expected);
        assert!(surreal.is_migrated().await.unwrap());
        for (company_node_id, mail, signature) in [
            (None, email("a@b.cr"), personal_signature),
            (company, email("c@d.cr"), company_signature),
        ] {
            let original = surreal
                .get_email_registration_for_node_id(&node_id, &company_node_id)
                .await
                .unwrap()
                .expect("surreal registration");
            let copied = target
                .get_email_registration_for_node_id(&node_id, &company_node_id)
                .await
                .unwrap()
                .expect("postgres registration");
            assert_eq!(copied.node_id, node_id);
            assert_eq!(copied.company_node_id, company_node_id);
            assert_eq!(copied.email, mail);
            assert_eq!(copied.mint_signature, signature);
            // PostgreSQL stores microseconds
            assert_eq!(
                copied.created_at.unix_timestamp_nanos() / 1000,
                original.created_at.unix_timestamp_nanos() / 1000
            );
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_copy_into_non_empty_target_fails(pool: ::sqlx::PgPool) {
        let surreal = init_surreal_db().await;
        let target = sql::DB::from_pool(pool);
        let node_id = random_node_id();
        surreal
            .insert_email_registration_for_node_id(
                &node_id,
                &None,
                &email("a@b.cr"),
                &random_signature(),
            )
            .await
            .unwrap();
        target
            .insert_email_registration_for_node_id(
                &node_id,
                &None,
                &email("c@d.cr"),
                &random_signature(),
            )
            .await
            .unwrap();

        assert!(copy(&surreal, Some(&target)).await.is_err());
        // SurrealDB stays writable and the existing row is untouched
        assert!(!surreal.is_migrated().await.unwrap());
        let existing = target
            .get_email_registration_for_node_id(&node_id, &None)
            .await
            .unwrap()
            .expect("registration");
        assert_eq!(existing.email, email("c@d.cr"));
    }
}
