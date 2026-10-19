use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::modules::identity::domain::{User, IDLE_TIMEOUT_MINS, SESSION_TTL_HOURS};

#[derive(sqlx::FromRow)]
struct Credentials {
    #[sqlx(flatten)]
    user: User,
    password_hash: String,
}

/// Active users of active tenants only.
pub async fn find_credentials(
    db: &PgPool,
    email: &str,
) -> Result<Option<(User, String)>, sqlx::Error> {
    let row: Option<Credentials> = sqlx::query_as(
        "SELECT u.id, u.tenant_id, u.email, u.full_name, t.is_master, u.password_hash
         FROM users u
         JOIN tenants t ON t.id = u.tenant_id
         WHERE u.email = $1 AND u.is_active AND t.status = 'active'",
    )
    .bind(email)
    .fetch_optional(db)
    .await?;

    Ok(row.map(|c| (c.user, c.password_hash)))
}

pub async fn insert_session(
    db: &PgPool,
    token_hash: &str,
    user_id: Uuid,
    user_agent: Option<&str>,
) -> Result<DateTime<Utc>, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO auth_sessions (token_hash, user_id, user_agent, expires_at)
         VALUES ($1, $2, $3, now() + make_interval(hours => $4))
         RETURNING expires_at",
    )
    .bind(token_hash)
    .bind(user_id)
    .bind(user_agent)
    .bind(SESSION_TTL_HOURS)
    .fetch_one(db)
    .await
}

/// Looks the session up and bumps `last_activity_at` in one round trip.
pub async fn touch_session(db: &PgPool, token_hash: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE auth_sessions s
         SET last_activity_at = now()
         FROM users u
         JOIN tenants t ON t.id = u.tenant_id
         WHERE s.token_hash = $1
           AND u.id = s.user_id
           AND u.is_active
           AND t.status = 'active'
           AND s.expires_at > now()
           AND s.last_activity_at > now() - make_interval(mins => $2)
         RETURNING u.id, u.tenant_id, u.email, u.full_name, t.is_master",
    )
    .bind(token_hash)
    .bind(IDLE_TIMEOUT_MINS)
    .fetch_optional(db)
    .await
}

pub async fn delete_session(db: &PgPool, token_hash: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM auth_sessions WHERE token_hash = $1")
        .bind(token_hash)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn delete_stale_sessions(db: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM auth_sessions
         WHERE expires_at <= now()
            OR last_activity_at <= now() - make_interval(mins => $1)",
    )
    .bind(IDLE_TIMEOUT_MINS)
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

pub async fn email_taken(db: &PgPool, email: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE email = $1)")
        .bind(email)
        .fetch_one(db)
        .await
}

pub async fn insert_master_user(
    db: &PgPool,
    email: &str,
    password_hash: &str,
    full_name: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO users (tenant_id, email, password_hash, full_name)
         SELECT id, $1, $2, $3 FROM tenants WHERE is_master
         ON CONFLICT (email) DO NOTHING",
    )
    .bind(email)
    .bind(password_hash)
    .bind(full_name)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}
