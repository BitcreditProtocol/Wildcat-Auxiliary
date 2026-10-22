use sqlx::{PgPool, postgres::PgPoolOptions};

pub mod sql {
    use super::*;
    /// `max_connections == 0` means Postgres is not configured.
    #[derive(Debug, Clone, Default, serde::Deserialize)]
    pub struct DBConnConfig {
        pub connection: String,
        pub max_connections: u32,
    }

    pub async fn connect(cfg: &DBConnConfig) -> sqlx::Result<PgPool> {
        PgPoolOptions::new()
            .max_connections(cfg.max_connections)
            .connect(&cfg.connection)
            .await
    }

    /// Folder the Dockerfiles fill with the shared + service migrations
    const MIGRATIONS_PATH: &str = "./migrations";

    pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
        let migrator = sqlx::migrate::Migrator::new(std::path::Path::new(MIGRATIONS_PATH)).await?;
        migrator.run(pool).await
    }
}
