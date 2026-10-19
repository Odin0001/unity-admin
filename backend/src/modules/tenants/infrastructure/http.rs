use axum::{
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode},
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::{
    modules::{
        identity::CurrentUser,
        tenants::{
            application::{self, CreatedTenant},
            domain::Tenant,
        },
    },
    platform::{error::AppError, AppState},
};

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list).post(create))
}

/// Called by tenant deployments, not by people.
pub fn internal_router() -> Router<AppState> {
    Router::new().route("/tenant", get(current_tenant))
}

async fn list(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> Result<Json<Vec<Tenant>>, AppError> {
    Ok(Json(application::list(&state.db, &user).await?))
}

#[derive(Deserialize)]
struct CreateTenant {
    slug: String,
    name: String,
}

async fn create(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<CreateTenant>,
) -> Result<(StatusCode, Json<CreatedTenant>), AppError> {
    let created = application::create(&state.db, &user, &req.slug, &req.name).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// A tenant deployment that sent a valid `X-Tenant-Key`.
struct CallingTenant(Tenant);

impl FromRequestParts<AppState> for CallingTenant {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let key = parts
            .headers
            .get("x-tenant-key")
            .and_then(|v| v.to_str().ok())
            .ok_or(AppError::Unauthorized)?;
        application::authenticate(&state.db, key).await.map(Self)
    }
}

async fn current_tenant(CallingTenant(tenant): CallingTenant) -> Json<Tenant> {
    Json(tenant)
}
