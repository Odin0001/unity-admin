use chrono::Utc;
use sqlx::PgPool;

use super::{
    domain::{Heartbeat, TenantHealth},
    infrastructure::postgres,
};
use crate::{
    modules::{identity::domain::User, tenants::domain::Tenant},
    platform::error::AppError,
};

pub async fn record_heartbeat(
    db: &PgPool,
    tenant: &Tenant,
    beat: &Heartbeat,
) -> Result<(), AppError> {
    if beat.version.trim().is_empty() || beat.version.len() > 64 {
        return Err(AppError::Invalid("version must be 1-64 characters"));
    }
    Ok(postgres::record(db, tenant.id, beat).await?)
}

pub async fn overview(db: &PgPool, actor: &User) -> Result<Vec<TenantHealth>, AppError> {
    actor.require_master()?;
    Ok(postgres::overview(db, Utc::now()).await?)
}
