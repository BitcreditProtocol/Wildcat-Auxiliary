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
}
