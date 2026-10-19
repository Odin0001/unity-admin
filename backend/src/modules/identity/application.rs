use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

use super::{
    domain::{normalize_email, User, MIN_PASSWORD_LEN},
    infrastructure::postgres,
};
use crate::platform::{error::AppError, password, token};

/// What the client gets back from a successful sign-in. `token` is the only
/// copy of the raw token anywhere.
#[derive(Debug, Serialize)]
pub struct Session {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub user: User,
}

pub async fn login(
    db: &PgPool,
    email: &str,
    password: String,
    user_agent: Option<&str>,
) -> Result<Session, AppError> {
    let (user, password_hash) = postgres::find_credentials(db, &normalize_email(email))
        .await?
        .ok_or(AppError::InvalidCredentials)?;

    if !password::verify(password, password_hash).await? {
        return Err(AppError::InvalidCredentials);
    }

    let token = token::generate();
    let expires_at =
        postgres::insert_session(db, &token::hash(&token), user.id, user_agent).await?;

    Ok(Session {
        token,
        expires_at,
        user,
    })
}

/// Fails for unknown, expired or idle sessions, and for users or tenants
/// that were disabled after signing in.
pub async fn authenticate(db: &PgPool, token: &str) -> Result<User, AppError> {
    postgres::touch_session(db, &token::hash(token))
        .await?
        .ok_or(AppError::Unauthorized)
}

pub async fn logout(db: &PgPool, token: &str) -> Result<(), AppError> {
    postgres::delete_session(db, &token::hash(token)).await?;
    Ok(())
}

pub async fn purge_expired_sessions(db: &PgPool) -> Result<u64, AppError> {
    Ok(postgres::delete_stale_sessions(db).await?)
}

/// Startup hook: creates the super user from `SUPER_USER_EMAIL` /
/// `SUPER_USER_PASSWORD` when both are set.
pub async fn ensure_super_user(db: &PgPool) -> Result<(), AppError> {
    // compose turns unset variables into empty strings
    let var = |name| std::env::var(name).ok().filter(|v| !v.is_empty());
    let (Some(email), Some(password)) = (var("SUPER_USER_EMAIL"), var("SUPER_USER_PASSWORD"))
    else {
        return Ok(());
    };

    if create_super_user(db, &email, password).await? {
        tracing::info!(%email, "created super user");
    }
    Ok(())
}

/// The first master-tenant account, so a fresh database has someone who
/// can sign in. Returns `false` if the email is already taken; that user's
/// password is left alone.
pub async fn create_super_user(
    db: &PgPool,
    email: &str,
    password: String,
) -> Result<bool, AppError> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AppError::Invalid(
            "SUPER_USER_PASSWORD must be at least 12 characters",
        ));
    }

    let email = normalize_email(email);
    if postgres::email_taken(db, &email).await? {
        return Ok(false);
    }

    let password_hash = password::hash(password).await?;
    Ok(postgres::insert_master_user(db, &email, &password_hash, "Super User").await?)
}
