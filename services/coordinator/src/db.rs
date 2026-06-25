use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Opens a connection pool to the PostgreSQL coordinator database.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(database_url)
        .await
}

/// Runs all pending up-migrations from services/coordinator/migrations/.
/// Safe to call on every startup — sqlx tracks which migrations have run.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}
