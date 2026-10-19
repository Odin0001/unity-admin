use serde::Serialize;
use sqlx::PgPool;

use super::{
    domain::{is_valid_slug, Tenant},
    infrastructure::postgres,
};
use crate::{
    modules::identity::domain::User,
    platform::{error::AppError, token},
};

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

    let api_key = token::generate();
    let tenant = postgres::insert(db, slug, name, &token::hash(&api_key))
        .await?
        .ok_or(AppError::Conflict("a tenant with this slug already exists"))?;

    tracing::info!(tenant_id = %tenant.id, slug, created_by = %actor.id, "tenant created");
    Ok(CreatedTenant { tenant, api_key })
}

/// Resolves the tenant behind an API key and records that it checked in.
pub async fn authenticate(db: &PgPool, api_key: &str) -> Result<Tenant, AppError> {
    postgres::touch_by_api_key(db, &token::hash(api_key))
        .await?
        .ok_or(AppError::Unauthorized)
}
