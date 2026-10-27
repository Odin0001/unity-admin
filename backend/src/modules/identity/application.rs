use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

use super::{
    domain::{
        check_new_password, normalize_email, User, UserSummary, INVITATION_TTL_MINS,
        LOGIN_FAILURES_PER_EMAIL, LOGIN_FAILURES_PER_IP, MIN_PASSWORD_LEN, RESET_LINK_TTL_MINS,
        RESET_REQUESTS_PER_EMAIL, RESET_REQUESTS_PER_IP,
    },
    infrastructure::postgres,
};
use crate::platform::{
    error::AppError, mail::Email, password, rate_limit::RateLimiter, token, AppState,
};

/// What the client gets back from a successful sign-in. `token` is the only
/// copy of the raw token anywhere.
#[derive(Debug, Serialize)]
pub struct Session {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub user: User,
}

/// Wrong passwords count against both the caller's address and the email;
/// past either limit, sign-in is refused until the window ends.
pub async fn login(
    db: &PgPool,
    limiter: &RateLimiter,
    client_ip: &str,
    email: &str,
    password: String,
    user_agent: Option<&str>,
) -> Result<Session, AppError> {
    let email = normalize_email(email);
    limiter.check(LOGIN_FAILURES_PER_IP, client_ip).await?;
    limiter.check(LOGIN_FAILURES_PER_EMAIL, &email).await?;

    let session = start_session(db, &email, password, user_agent).await;
    if matches!(session, Err(AppError::InvalidCredentials)) {
        limiter.hit(LOGIN_FAILURES_PER_IP, client_ip).await;
        limiter.hit(LOGIN_FAILURES_PER_EMAIL, &email).await;
    }
    session
}

async fn start_session(
    db: &PgPool,
    email: &str,
    password: String,
    user_agent: Option<&str>,
) -> Result<Session, AppError> {
    let (user, password_hash) = postgres::find_credentials(db, email)
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

/// Clears ended sessions and expired password links. Returns how many
/// sessions went.
pub async fn purge_expired_sessions(db: &PgPool) -> Result<u64, AppError> {
    postgres::delete_expired_resets(db).await?;
    Ok(postgres::delete_stale_sessions(db).await?)
}

/// Emails a one-time link for choosing a new password. Says nothing about
/// whether the email has an account: the caller gets the same answer
/// either way.
pub async fn request_password_reset(
    state: &AppState,
    client_ip: &str,
    email: &str,
) -> Result<(), AppError> {
    let email = normalize_email(email);
    state
        .limiter
        .count(RESET_REQUESTS_PER_IP, client_ip)
        .await?;
    state
        .limiter
        .count(RESET_REQUESTS_PER_EMAIL, &email)
        .await?;

    let Some(user) = postgres::find_active(&state.db, &email).await? else {
        return Ok(());
    };
    let link = issue_link(state, &user, RESET_LINK_TTL_MINS).await?;
    state.mailer.send(Email {
        to: user.email,
        subject: "Reset your password".into(),
        body: format!(
            "Someone asked to reset the password for your platform admin account.\n\n\
             Choose a new one here. The link works once, for the next hour:\n{link}\n\n\
             If that wasn't you, ignore this email and your password stays as it is."
        ),
    });
    Ok(())
}

/// Sets a new password from an emailed link and signs the user out
/// everywhere.
pub async fn reset_password(
    db: &PgPool,
    token: &str,
    new_password: String,
) -> Result<(), AppError> {
    check_new_password(&new_password)?;
    let password_hash = password::hash(new_password).await?;
    if postgres::reset_password(db, &token::hash(token), &password_hash).await? {
        Ok(())
    } else {
        Err(AppError::Invalid("this link is invalid or has expired"))
    }
}

/// For someone signed in who knows their current password. Their other
/// sessions end; this one stays.
pub async fn change_password(
    db: &PgPool,
    user: &User,
    session_token: &str,
    current_password: String,
    new_password: String,
) -> Result<(), AppError> {
    check_new_password(&new_password)?;
    if current_password == new_password {
        return Err(AppError::Invalid("the new password must be different"));
    }
    let Some(current_hash) = postgres::password_hash(db, user.id).await? else {
        return Err(AppError::InvalidCredentials);
    };
    if !password::verify(current_password, current_hash).await? {
        return Err(AppError::InvalidCredentials);
    }

    let password_hash = password::hash(new_password).await?;
    postgres::change_password(db, user.id, &password_hash, &token::hash(session_token)).await?;
    Ok(())
}

pub async fn list_super_users(db: &PgPool, actor: &User) -> Result<Vec<UserSummary>, AppError> {
    actor.require_master()?;
    Ok(postgres::list_master_users(db).await?)
}

/// Adds another super user. They get an email with a link to set their own
/// password, so nobody else ever knows it.
pub async fn invite_super_user(
    state: &AppState,
    actor: &User,
    email: &str,
    full_name: &str,
) -> Result<User, AppError> {
    actor.require_master()?;
    let email = normalize_email(email);
    let full_name = full_name.trim();
    if !email.contains('@') {
        return Err(AppError::Invalid("email is not valid"));
    }
    if full_name.is_empty() {
        return Err(AppError::Invalid("full_name is required"));
    }

    let user = postgres::insert_invited(&state.db, &email, full_name)
        .await?
        .ok_or(AppError::Conflict("a user with this email already exists"))?;
    let link = issue_link(state, &user, INVITATION_TTL_MINS).await?;
    state.mailer.send(Email {
        to: user.email.clone(),
        subject: "You've been added to the platform admin console".into(),
        body: format!(
            "{} added you as a super user of the platform admin console.\n\n\
             Set your password here. The link works once, for the next 3 days:\n{link}",
            actor.full_name
        ),
    });

    tracing::info!(user_id = %user.id, invited_by = %actor.id, "super user invited");
    Ok(user)
}

async fn issue_link(state: &AppState, user: &User, ttl_mins: i32) -> Result<String, AppError> {
    let token = token::generate();
    postgres::insert_reset(&state.db, &token::hash(&token), user.id, ttl_mins).await?;
    Ok(format!("{}/reset-password?token={token}", state.public_url))
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
