use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};

use crate::{
    modules::{
        identity::CurrentUser,
        monitoring::{
            application,
            domain::{Heartbeat, TenantHealth},
        },
        tenants::CallingTenant,
    },
    platform::{error::AppError, AppState},
};

pub fn router() -> Router<AppState> {
    Router::new().route("/monitoring", get(overview))
}

/// Called by tenant deployments, not by people.
pub fn internal_router() -> Router<AppState> {
    Router::new().route("/heartbeat", post(heartbeat))
}

async fn overview(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> Result<Json<Vec<TenantHealth>>, AppError> {
    Ok(Json(application::overview(&state.db, &user).await?))
}

async fn heartbeat(
    State(state): State<AppState>,
    CallingTenant(tenant): CallingTenant,
    Json(beat): Json<Heartbeat>,
) -> Result<StatusCode, AppError> {
    application::record_heartbeat(&state.db, &tenant, &beat).await?;
    Ok(StatusCode::NO_CONTENT)
}
