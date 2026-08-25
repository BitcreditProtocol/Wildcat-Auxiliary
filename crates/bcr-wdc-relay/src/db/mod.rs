use deadpool_postgres::Pool;

pub struct PostgresStore {
    pub pool: Pool,
}

impl PostgresStore {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Creates the tables, if they don't exist yet
    pub async fn init(&self) -> Result<(), anyhow::Error> {
        // Reuse the relay database's pinned migrations instead of maintaining a second schema.
        nostr_postgres_db::run_migrations(&self.pool).await?;

        // File Store
        let qry = r#"
            CREATE TABLE IF NOT EXISTS files (
                hash CHAR(64) PRIMARY KEY,
                data BYTEA NOT NULL,
                size INTEGER NOT NULL
            )
        "#;
        self.pool.get().await?.execute(qry, &[]).await?;
        Ok(())
    }

    pub async fn is_ready(&self) -> Result<bool, anyhow::Error> {
        let row = self
            .pool
            .get()
            .await?
            .query_one(
                "SELECT to_regclass('public.events') IS NOT NULL AND to_regclass('public.event_tags') IS NOT NULL AND to_regclass('public.files') IS NOT NULL",
                &[],
            )
            .await?;
        Ok(row.get(0))
    }
}
