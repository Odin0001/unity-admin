use sqlx::{PgConnection, PgPool};

use crate::{
    modules::tenants::domain::{Tenant, TenantStatus},
    shared_kernel::TenantId,
};

const COLUMNS: &str = "id, slug, name, status, is_master, pin, api_url, last_seen_at, created_at";

pub async fn list(db: &PgPool) -> Result<Vec<Tenant>, sqlx::Error> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM tenants ORDER BY created_at"
    ))
    .fetch_all(db)
    .await
}

/// `None` when the slug or the PIN is taken.
pub async fn insert(
    db: &PgPool,
    slug: &str,
    name: &str,
    pin: &str,
    api_url: Option<&str>,
    api_key_hash: &str,
) -> Result<Option<Tenant>, sqlx::Error> {
    sqlx::query_as(&format!(
        "INSERT INTO tenants (slug, name, pin, api_url, api_key_hash) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT DO NOTHING
         RETURNING {COLUMNS}"
    ))
    .bind(slug)
    .bind(name)
    .bind(pin)
    .bind(api_url)
    .bind(api_key_hash)
    .fetch_optional(db)
    .await
}

pub async fn slug_taken(db: &PgPool, slug: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM tenants WHERE slug = $1)")
        .bind(slug)
        .fetch_one(db)
        .await
}

pub async fn find_by_pin(db: &PgPool, pin: &str) -> Result<Option<Tenant>, sqlx::Error> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM tenants WHERE pin = $1"))
        .bind(pin)
        .fetch_optional(db)
        .await
}

pub async fn find(db: &PgPool, id: TenantId) -> Result<Option<Tenant>, sqlx::Error> {
    sqlx::query_as(&format!("SELECT {COLUMNS} FROM tenants WHERE id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Fields left as `None` stay as they are.
pub async fn update(
    db: &PgPool,
    id: TenantId,
    name: Option<&str>,
    api_url: Option<&str>,
) -> Result<Option<Tenant>, sqlx::Error> {
    sqlx::query_as(&format!(
        "UPDATE tenants
         SET name = COALESCE($2, name), api_url = COALESCE($3, api_url), updated_at = now()
         WHERE id = $1
         RETURNING {COLUMNS}"
    ))
    .bind(id)
    .bind(name)
    .bind(api_url)
    .fetch_optional(db)
    .await
}

/// The status is part of the allowances document, so its version goes up
/// with it.
pub async fn set_status(
    db: &PgPool,
    id: TenantId,
    status: TenantStatus,
) -> Result<Option<Tenant>, sqlx::Error> {
    let mut tx = db.begin().await?;
    let tenant = sqlx::query_as(&format!(
        "UPDATE tenants SET status = $2, updated_at = now() WHERE id = $1 RETURNING {COLUMNS}"
    ))
    .bind(id)
    .bind(status)
    .fetch_optional(&mut *tx)
    .await?;
    bump_allowances(&mut tx, id).await?;
    tx.commit().await?;
    Ok(tenant)
}

/// Same as the status: the PIN is in the allowances document. `None` when
/// the tenant is gone or the new PIN is taken.
pub async fn set_pin(db: &PgPool, id: TenantId, pin: &str) -> Result<Option<Tenant>, sqlx::Error> {
    let mut tx = db.begin().await?;
    let tenant = sqlx::query_as(&format!(
        "UPDATE tenants SET pin = $2, updated_at = now()
         WHERE id = $1 AND NOT EXISTS (SELECT 1 FROM tenants WHERE pin = $2)
         RETURNING {COLUMNS}"
    ))
    .bind(id)
    .bind(pin)
    .fetch_optional(&mut *tx)
    .await?;
    bump_allowances(&mut tx, id).await?;
    tx.commit().await?;
    Ok(tenant)
}

async fn bump_allowances(conn: &mut PgConnection, id: TenantId) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE subscriptions SET version = version + 1 WHERE tenant_id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
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
