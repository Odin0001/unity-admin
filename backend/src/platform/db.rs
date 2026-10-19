use std::time::Duration;

use sqlx::{postgres::PgPoolOptions, PgPool};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("DATABASE_URL is not set")]
    MissingUrl,
    #[error("failed to connect to postgres: {0}")]
    Connect(#[source] sqlx::Error),
    #[error("failed to run migrations: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

pub async fn connect() -> Result<PgPool, DbError> {
    let url = std::env::var("DATABASE_URL").map_err(|_| DbError::MissingUrl)?;

    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&url)
        .await
        .map_err(DbError::Connect)
}

pub async fn migrate(pool: &PgPool) -> Result<(), DbError> {
    sqlx::migrate!().run(pool).await?;
    Ok(())
}
