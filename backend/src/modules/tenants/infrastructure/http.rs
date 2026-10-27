use axum::{
    extract::{FromRequestParts, Path, State},
    http::{request::Parts, StatusCode},
    routing::{get, patch, post, put},
    Json, Router,
};
use serde::Deserialize;

use crate::{
    modules::{
        identity::CurrentUser,
        tenants::{
            application::{self, CreatedTenant, Directory},
            domain::{Tenant, TenantStatus},
        },
    },
    platform::{error::AppError, rate_limit::ClientIp, AppState},
    shared_kernel::TenantId,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", patch(update))
        .route("/{id}/status", put(set_status))
        .route("/{id}/pin", post(rotate_pin))
}

/// Public: the shared login page calls it before anyone has signed in.
pub fn directory_router() -> Router<AppState> {
    Router::new().route("/lookup", post(lookup))
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
    api_url: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<CreateTenant>,
) -> Result<(StatusCode, Json<CreatedTenant>), AppError> {
    let created = application::create(
        &state.db,
        &user,
        &req.slug,
        &req.name,
        req.api_url.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Deserialize)]
struct UpdateTenant {
    name: Option<String>,
    api_url: Option<String>,
}

async fn update(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<TenantId>,
    Json(req): Json<UpdateTenant>,
) -> Result<Json<Tenant>, AppError> {
    let tenant = application::update(
        &state.db,
        &user,
        id,
        req.name.as_deref(),
        req.api_url.as_deref(),
    )
    .await?;
    Ok(Json(tenant))
}

#[derive(Deserialize)]
struct SetStatus {
    status: TenantStatus,
}

async fn set_status(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<TenantId>,
    Json(req): Json<SetStatus>,
) -> Result<Json<Tenant>, AppError> {
    let tenant =
        application::set_status(&state.db, &state.tenant_client, &user, id, req.status).await?;
    Ok(Json(tenant))
}

async fn rotate_pin(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<TenantId>,
) -> Result<Json<Tenant>, AppError> {
    let tenant = application::rotate_pin(&state.db, &state.tenant_client, &user, id).await?;
    Ok(Json(tenant))
}

#[derive(Deserialize)]
struct Lookup {
    pin: String,
}

async fn lookup(
    State(state): State<AppState>,
    client_ip: ClientIp,
    Json(req): Json<Lookup>,
) -> Result<Json<Directory>, AppError> {
    let found = application::lookup(&state.db, &state.limiter, &client_ip.key(), &req.pin).await?;
    Ok(Json(found))
}

/// A tenant deployment that sent a valid `X-Tenant-Key`.
pub struct CallingTenant(pub Tenant);

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
