use sqlx::PgPool;

use crate::modules::tenants::domain::Tenant;

const COLUMNS: &str = "id, slug, name, status, is_master, last_seen_at, created_at";

pub async fn list(db: &PgPool) -> Result<Vec<Tenant>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM tenants ORDER BY created_at"
    ))
    .fetch_all(db)
    .await
}

/// `None` when the slug is taken.
pub async fn insert(
    db: &PgPool,
    slug: &str,
    name: &str,
    api_key_hash: &str,
) -> Result<Option<Tenant>, sqlx::Error> {
    sqlx::query_as(&format!(
        "INSERT INTO tenants (slug, name, api_key_hash) VALUES ($1, $2, $3)
         ON CONFLICT (slug) DO NOTHING
         RETURNING {COLUMNS}"
    ))
    .bind(slug)
    .bind(name)
    .bind(api_key_hash)
    .fetch_optional(db)
    .await
}

pub async fn touch_by_api_key(
    db: &PgPool,
    api_key_hash: &str,
) -> Result<Option<Tenant>, sqlx::Error> {
    sqlx::query_as(&format!(
        "UPDATE tenants SET last_seen_at = now() WHERE api_key_hash = $1 RETURNING {COLUMNS}"
    ))
    .bind(api_key_hash)
    .fetch_optional(db)
    .await
}
