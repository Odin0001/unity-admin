use serde::Serialize;
use sqlx::PgPool;

use super::{
    domain::{
        generate_pin, is_valid_slug, normalize_api_url, normalize_pin, Tenant, TenantStatus,
        LOOKUPS_PER_IP,
    },
    infrastructure::postgres,
    TenantClient,
};
use crate::{
    modules::identity::domain::User,
    platform::{error::AppError, rate_limit::RateLimiter, token},
    shared_kernel::TenantId,
};

/// A fresh PIN collides about once in a billion; a few tries is plenty.
const PIN_ATTEMPTS: usize = 5;

#[derive(Debug, Serialize)]
pub struct CreatedTenant {
    pub tenant: Tenant,
    /// Returned once, never stored in the clear.
    pub api_key: String,
}

pub async fn list(db: &PgPool, actor: &User) -> Result<Vec<Tenant>, AppError> {
    actor.require_master()?;
    Ok(postgres::list(db).await?)
}

pub async fn create(
    db: &PgPool,
    actor: &User,
    slug: &str,
    name: &str,
    api_url: Option<&str>,
) -> Result<CreatedTenant, AppError> {
    actor.require_master()?;

    let (slug, name) = (slug.trim(), name.trim());
    if !is_valid_slug(slug) {
        return Err(AppError::Invalid(
            "slug must be 2-63 lowercase letters, digits or dashes",
        ));
    }
    if name.is_empty() {
        return Err(AppError::Invalid("name is required"));
    }
    let api_url = api_url.map(valid_api_url).transpose()?;

    let api_key = token::generate();
    let api_key_hash = token::hash(&api_key);
    for _ in 0..PIN_ATTEMPTS {
        let pin = generate_pin();
        if let Some(tenant) =
            postgres::insert(db, slug, name, &pin, api_url.as_deref(), &api_key_hash).await?
        {
            tracing::info!(tenant_id = %tenant.id, slug, created_by = %actor.id, "tenant created");
            return Ok(CreatedTenant { tenant, api_key });
        }
        if postgres::slug_taken(db, slug).await? {
            return Err(AppError::Conflict("a tenant with this slug already exists"));
        }
    }
    Err(AppError::Conflict("couldn't find a free PIN, try again"))
}

/// Gives the tenant a new PIN, for example after the old one was shared too
/// widely. The old one stops working at once, on admin and on the tenant.
pub async fn rotate_pin(
    db: &PgPool,
    client: &TenantClient,
    actor: &User,
    id: TenantId,
) -> Result<Tenant, AppError> {
    actor.require_master()?;
    if find(db, id).await?.is_master {
        return Err(AppError::Invalid("the master tenant has no PIN"));
    }

    for _ in 0..PIN_ATTEMPTS {
        if let Some(tenant) = postgres::set_pin(db, id, &generate_pin()).await? {
            tracing::info!(tenant_id = %id, by = %actor.id, "tenant PIN rotated");
            push_allowances(client, &tenant);
            return Ok(tenant);
        }
    }
    Err(AppError::Conflict("couldn't find a free PIN, try again"))
}

#[derive(Debug, Serialize)]
pub struct Directory {
    pub name: String,
    pub login_url: String,
}

/// The shared login page: turns a PIN into the tenant's own sign-in page.
/// Unknown PINs, tenants that can't be used right now, and tenants not
/// deployed yet all look the same from outside.
pub async fn lookup(
    db: &PgPool,
    limiter: &RateLimiter,
    client_ip: &str,
    pin: &str,
) -> Result<Directory, AppError> {
    limiter.count(LOOKUPS_PER_IP, client_ip).await?;

    let pin = normalize_pin(pin).ok_or(AppError::NotFound)?;
    let tenant = postgres::find_by_pin(db, &pin)
        .await?
        .filter(|t| t.status.is_live())
        .ok_or(AppError::NotFound)?;
    let api_url = tenant.api_url.ok_or(AppError::NotFound)?;

    Ok(Directory {
        name: tenant.name,
        login_url: format!("{api_url}/login?pin={pin}"),
    })
}

pub async fn update(
    db: &PgPool,
    actor: &User,
    id: TenantId,
    name: Option<&str>,
    api_url: Option<&str>,
) -> Result<Tenant, AppError> {
    actor.require_master()?;

    let name = name.map(str::trim);
    if name == Some("") {
        return Err(AppError::Invalid("name can't be empty"));
    }
    let api_url = api_url.map(valid_api_url).transpose()?;

    postgres::update(db, id, name, api_url.as_deref())
        .await?
        .ok_or(AppError::NotFound)
}

/// Moves a tenant between active, past due, suspended and cancelled, and
/// tells its deployment straight away.
pub async fn set_status(
    db: &PgPool,
    client: &TenantClient,
    actor: &User,
    id: TenantId,
    status: TenantStatus,
) -> Result<Tenant, AppError> {
    actor.require_master()?;
    if find(db, id).await?.is_master {
        return Err(AppError::Invalid("the master tenant's status can't change"));
    }

    let tenant = postgres::set_status(db, id, status)
        .await?
        .ok_or(AppError::NotFound)?;
    tracing::info!(tenant_id = %id, ?status, by = %actor.id, "tenant status changed");
    push_allowances(client, &tenant);
    Ok(tenant)
}

/// Asks the deployment to fetch its allowances now rather than at its next
/// 5-minute refresh. Runs in the background: a deployment that can't be
/// reached still catches up on its own.
pub fn push_allowances(client: &TenantClient, tenant: &Tenant) {
    let Some(api_url) = tenant.api_url.clone() else {
        return;
    };
    let (client, tenant_id) = (client.clone(), tenant.id);
    tokio::spawn(async move {
        match client.refresh_allowances(&api_url).await {
            Ok(()) => tracing::info!(%tenant_id, "tenant refreshed its allowances"),
            Err(err) => {
                tracing::warn!(%tenant_id, %err, "couldn't reach tenant; it catches up within 5 minutes");
            }
        }
    });
}

/// For other modules, after they've done their own access checks.
pub async fn find(db: &PgPool, id: TenantId) -> Result<Tenant, AppError> {
    postgres::find(db, id).await?.ok_or(AppError::NotFound)
}

/// Resolves the tenant behind an API key and records that it checked in.
pub async fn authenticate(db: &PgPool, api_key: &str) -> Result<Tenant, AppError> {
    postgres::touch_by_api_key(db, &token::hash(api_key))
        .await?
        .ok_or(AppError::Unauthorized)
}

fn valid_api_url(url: &str) -> Result<String, AppError> {
    normalize_api_url(url).ok_or(AppError::Invalid(
        "api_url must be an http:// or https:// address",
    ))
}
