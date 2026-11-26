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
        // File Store
        let qry = r#"
            CREATE TABLE IF NOT EXISTS files (
                hash CHAR(64) PRIMARY KEY,
                data BYTEA NOT NULL,
                size INTEGER NOT NULL
            )
        "#;
        self.pool.get().await?.execute(qry, &[]).await?;

        // Push devices: one token per key and platform
        let qry = r#"
            CREATE TABLE IF NOT EXISTS push_devices (
                pubkey CHAR(64) NOT NULL,
                platform TEXT NOT NULL CHECK (platform IN ('android', 'ios')),
                token TEXT NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                PRIMARY KEY (pubkey, platform)
            )
        "#;
        self.pool.get().await?.execute(qry, &[]).await?;
        Ok(())
    }
}
