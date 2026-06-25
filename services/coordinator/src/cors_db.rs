//! Database-backed CORS configuration
//!
//! Issue #261: Add coordinator CORS configuration for production frontend domains
//! Move from permissive CORS to a strict allowlist of frontend domains.
//! Configurable via environment variable and database.

use sqlx::PgPool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CorsConfig {
    pub id: i64,
    pub origin: String,
    pub enabled: bool,
    pub description: Option<String>,
}

/// Load all enabled CORS origins from the database
pub async fn load_cors_origins(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let configs = sqlx::query_as::<_, CorsConfig>(
        "SELECT id, origin, enabled, description 
         FROM cors_configs 
         WHERE enabled = true",
    )
    .fetch_all(pool)
    .await?;

    Ok(configs.into_iter().map(|c| c.origin).collect())
}

/// Check if an origin is allowed
pub async fn is_origin_allowed(pool: &PgPool, origin: &str) -> Result<bool, sqlx::Error> {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM cors_configs WHERE origin = $1 AND enabled = true")
            .bind(origin)
            .fetch_one(pool)
            .await?;

    Ok(count.0 > 0)
}

/// Add or update a CORS origin
pub async fn upsert_cors_origin(
    pool: &PgPool,
    origin: &str,
    enabled: bool,
    description: Option<&str>,
) -> Result<CorsConfig, sqlx::Error> {
    sqlx::query_as::<_, CorsConfig>(
        "INSERT INTO cors_configs (origin, enabled, description) 
         VALUES ($1, $2, $3) 
         ON CONFLICT (origin) 
         DO UPDATE SET 
             enabled = EXCLUDED.enabled, 
             description = EXCLUDED.description,
             updated_at = NOW()
         RETURNING id, origin, enabled, description",
    )
    .bind(origin)
    .bind(enabled)
    .bind(description)
    .fetch_one(pool)
    .await
}

/// Delete a CORS origin
pub async fn delete_cors_origin(pool: &PgPool, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM cors_configs WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// List all CORS configurations
pub async fn list_cors_configs(pool: &PgPool) -> Result<Vec<CorsConfig>, sqlx::Error> {
    sqlx::query_as::<_, CorsConfig>(
        "SELECT id, origin, enabled, description 
         FROM cors_configs 
         ORDER BY id",
    )
    .fetch_all(pool)
    .await
}

/// Load CORS origins from environment variable (comma-separated)
pub fn cors_origins_from_env() -> Vec<String> {
    std::env::var("CORS_ALLOWED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Merge CORS origins from environment and database
pub async fn get_effective_cors_origins(pool: Option<&PgPool>) -> Vec<String> {
    let mut origins = cors_origins_from_env();

    if let Some(db) = pool {
        if let Ok(db_origins) = load_cors_origins(db).await {
            for origin in db_origins {
                if !origins.contains(&origin) {
                    origins.push(origin);
                }
            }
        }
    }

    // If no origins configured, default to permissive for development
    if origins.is_empty() {
        tracing::warn!("No CORS origins configured, defaulting to permissive mode");
        origins.push("*".to_string());
    }

    origins
}
