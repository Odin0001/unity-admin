use chrono::{Duration, Utc};
use sqlx::PgPool;

use super::{
    domain::{Allowances, BillingInterval, Plan, ALLOWANCES_TTL_HOURS},
    infrastructure::postgres,
};
use crate::{
    modules::{
        identity::domain::User,
        tenants::{self, TenantClient},
    },
    platform::error::AppError,
    shared_kernel::TenantId,
};

pub async fn list_plans(db: &PgPool, actor: &User) -> Result<Vec<Plan>, AppError> {
    actor.require_master()?;
    Ok(postgres::plans(db).await?)
}

/// Assigns a plan by hand and tells the tenant's deployment. Stripe will
/// drive this later; for now the platform owner does it from the console.
pub async fn subscribe(
    db: &PgPool,
    client: &TenantClient,
    actor: &User,
    tenant_id: TenantId,
    plan_code: &str,
    interval: BillingInterval,
) -> Result<Allowances, AppError> {
    actor.require_master()?;
    let tenant = tenants::application::find(db, tenant_id).await?;
    if tenant.is_master {
        return Err(AppError::Invalid("the master tenant doesn't have a plan"));
    }

    if !postgres::subscribe(db, tenant_id, plan_code.trim(), interval).await? {
        return Err(AppError::Invalid("unknown plan"));
    }

    tracing::info!(%tenant_id, plan = plan_code, ?interval, by = %actor.id, "subscription changed");
    let allowances = allowances(db, tenant_id).await?;
    tenants::application::push_allowances(client, &tenant);
    Ok(allowances)
}

pub async fn view_allowances(
    db: &PgPool,
    actor: &User,
    tenant_id: TenantId,
) -> Result<Allowances, AppError> {
    actor.require_master()?;
    allowances(db, tenant_id).await
}

/// The document a tenant enforces. 404 until the tenant has a plan.
pub async fn allowances(db: &PgPool, tenant_id: TenantId) -> Result<Allowances, AppError> {
    let valid_until = Utc::now() + Duration::hours(ALLOWANCES_TTL_HOURS);
    postgres::allowances(db, tenant_id, valid_until)
        .await?
        .ok_or(AppError::NotFound)
}
