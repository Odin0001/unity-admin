use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::modules::identity::domain::{User, UserSummary, IDLE_TIMEOUT_MINS, SESSION_TTL_HOURS};

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
         WHERE u.email = $1 AND u.is_active AND t.status = 'active'
           AND u.password_hash IS NOT NULL",
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

/// Anyone who could sign in, or could once they set a password.
pub async fn find_active(db: &PgPool, email: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id, u.tenant_id, u.email, u.full_name, t.is_master
         FROM users u
         JOIN tenants t ON t.id = u.tenant_id
         WHERE u.email = $1 AND u.is_active AND t.status = 'active'",
    )
    .bind(email)
    .fetch_optional(db)
    .await
}

pub async fn password_hash(db: &PgPool, user_id: Uuid) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(db)
        .await
}

/// Replaces any earlier link for the same user, so only the newest works.
pub async fn insert_reset(
    db: &PgPool,
    token_hash: &str,
    user_id: Uuid,
    ttl_mins: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "WITH older AS (DELETE FROM password_resets WHERE user_id = $2)
         INSERT INTO password_resets (token_hash, user_id, expires_at)
         VALUES ($1, $2, now() + make_interval(mins => $3))",
    )
    .bind(token_hash)
    .bind(user_id)
    .bind(ttl_mins)
    .execute(db)
    .await?;
    Ok(())
}

/// Uses up the link, sets the password and signs the user out everywhere.
/// `false` when the link is unknown, used or expired.
pub async fn reset_password(
    db: &PgPool,
    token_hash: &str,
    password_hash: &str,
) -> Result<bool, sqlx::Error> {
    let changed: Option<Uuid> = sqlx::query_scalar(
        "WITH used AS (
             DELETE FROM password_resets WHERE token_hash = $1 AND expires_at > now()
             RETURNING user_id
         ), changed AS (
             UPDATE users SET password_hash = $2, updated_at = now()
             WHERE id IN (SELECT user_id FROM used)
             RETURNING id
         ), signed_out AS (
             DELETE FROM auth_sessions WHERE user_id IN (SELECT id FROM changed)
         )
         SELECT id FROM changed",
    )
    .bind(token_hash)
    .bind(password_hash)
    .fetch_optional(db)
    .await?;
    Ok(changed.is_some())
}

/// Ends every other session of the user; the one making the change stays.
pub async fn change_password(
    db: &PgPool,
    user_id: Uuid,
    password_hash: &str,
    keep_session: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = $2, updated_at = now() WHERE id = $1")
        .bind(user_id)
        .bind(password_hash)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM auth_sessions WHERE user_id = $1 AND token_hash <> $2")
        .bind(user_id)
        .bind(keep_session)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM password_resets WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

pub async fn delete_expired_resets(db: &PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM password_resets WHERE expires_at <= now()")
        .execute(db)
        .await?;
    Ok(result.rows_affected())
}

/// A master-tenant user without a password yet. `None` if the email is
/// taken.
pub async fn insert_invited(
    db: &PgPool,
    email: &str,
    full_name: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO users (tenant_id, email, full_name)
         SELECT id, $1, $2 FROM tenants WHERE is_master
         ON CONFLICT (email) DO NOTHING
         RETURNING id, tenant_id, email, full_name, true AS is_master",
    )
    .bind(email)
    .bind(full_name)
    .fetch_optional(db)
    .await
}

pub async fn list_master_users(db: &PgPool) -> Result<Vec<UserSummary>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id, u.email, u.full_name, u.is_active,
                u.password_hash IS NOT NULL AS has_password, u.created_at
         FROM users u
         JOIN tenants t ON t.id = u.tenant_id
         WHERE t.is_master
         ORDER BY u.created_at",
    )
    .fetch_all(db)
    .await
}
