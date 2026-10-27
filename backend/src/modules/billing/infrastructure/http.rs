use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::{
    modules::{
        billing::{
            application,
            domain::{Allowances, BillingInterval, Plan},
        },
        identity::CurrentUser,
        tenants::CallingTenant,
    },
    platform::{error::AppError, AppState},
    shared_kernel::TenantId,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/plans", get(plans)).route(
        "/subscriptions/{tenant_id}",
        get(subscription).put(subscribe),
    )
}

pub fn internal_router() -> Router<AppState> {
    Router::new().route("/allowances", get(own_allowances))
}

async fn plans(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> Result<Json<Vec<Plan>>, AppError> {
    Ok(Json(application::list_plans(&state.db, &user).await?))
}

async fn subscription(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(tenant_id): Path<TenantId>,
) -> Result<Json<Allowances>, AppError> {
    Ok(Json(
        application::view_allowances(&state.db, &user, tenant_id).await?,
    ))
}

#[derive(Deserialize)]
struct Subscribe {
    plan: String,
    interval: BillingInterval,
}

async fn subscribe(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(tenant_id): Path<TenantId>,
    Json(req): Json<Subscribe>,
) -> Result<Json<Allowances>, AppError> {
    let allowances = application::subscribe(
        &state.db,
        &state.tenant_client,
        &user,
        tenant_id,
        &req.plan,
        req.interval,
    )
    .await?;
    Ok(Json(allowances))
}

async fn own_allowances(
    State(state): State<AppState>,
    CallingTenant(tenant): CallingTenant,
) -> Result<Json<Allowances>, AppError> {
    Ok(Json(application::allowances(&state.db, tenant.id).await?))
}
