use axum::{
    extract::{FromRequestParts, State},
    http::{header, request::Parts, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::{
    modules::identity::{
        application::{self, Session},
        domain::{User, UserSummary},
    },
    platform::{error::AppError, rate_limit::ClientIp, AppState},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/password", post(change_password))
        .route("/password-reset", post(request_password_reset))
        .route("/password-reset/confirm", post(reset_password))
}

/// Super users managing each other.
pub fn users_router() -> Router<AppState> {
    Router::new().route("/", get(list_users).post(invite_user))
}

/// Taking this as a handler argument is what makes a route require a
/// signed-in user (`Authorization: Bearer <token>`).
pub struct CurrentUser(pub User);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let token = bearer(&parts.headers).ok_or(AppError::Unauthorized)?;
        application::authenticate(&state.db, token).await.map(Self)
    }
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

async fn login(
    State(state): State<AppState>,
    client_ip: ClientIp,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Json<Session>, AppError> {
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());

    let session = application::login(
        &state.db,
        &state.limiter,
        &client_ip.key(),
        &req.email,
        req.password,
        user_agent,
    )
    .await?;
    tracing::info!(user_id = %session.user.id, "user signed in");
    Ok(Json(session))
}

// Unknown or already revoked tokens still get a 204.
async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<StatusCode, AppError> {
    let token = bearer(&headers).ok_or(AppError::Unauthorized)?;
    application::logout(&state.db, token).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(CurrentUser(user): CurrentUser) -> Json<User> {
    Json(user)
}

#[derive(Deserialize)]
struct ChangePassword {
    current_password: String,
    new_password: String,
}

async fn change_password(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    headers: HeaderMap,
    Json(req): Json<ChangePassword>,
) -> Result<StatusCode, AppError> {
    let token = bearer(&headers).ok_or(AppError::Unauthorized)?;
    application::change_password(
        &state.db,
        &user,
        token,
        req.current_password,
        req.new_password,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ResetRequest {
    email: String,
}

/// Always 202, whether or not the email has an account.
async fn request_password_reset(
    State(state): State<AppState>,
    client_ip: ClientIp,
    Json(req): Json<ResetRequest>,
) -> Result<StatusCode, AppError> {
    application::request_password_reset(&state, &client_ip.key(), &req.email).await?;
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
struct ResetPassword {
    token: String,
    password: String,
}

async fn reset_password(
    State(state): State<AppState>,
    Json(req): Json<ResetPassword>,
) -> Result<StatusCode, AppError> {
    application::reset_password(&state.db, &req.token, req.password).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_users(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> Result<Json<Vec<UserSummary>>, AppError> {
    Ok(Json(application::list_super_users(&state.db, &user).await?))
}

#[derive(Deserialize)]
struct InviteUser {
    email: String,
    full_name: String,
}

async fn invite_user(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(req): Json<InviteUser>,
) -> Result<(StatusCode, Json<User>), AppError> {
    let invited = application::invite_super_user(&state, &user, &req.email, &req.full_name).await?;
    Ok((StatusCode::CREATED, Json(invited)))
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn auth(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static(value));
        headers
    }

    #[test]
    fn reads_bearer_token() {
        assert_eq!(bearer(&auth("Bearer abc")), Some("abc"));
    }

    #[test]
    fn ignores_other_schemes_and_blank_tokens() {
        assert_eq!(bearer(&HeaderMap::new()), None);
        assert_eq!(bearer(&auth("Basic abc")), None);
        assert_eq!(bearer(&auth("bearer abc")), None);
        assert_eq!(bearer(&auth("Bearer    ")), None);
    }
}
