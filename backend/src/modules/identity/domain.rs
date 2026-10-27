use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    platform::{error::AppError, rate_limit::Rule},
    shared_kernel::TenantId,
};

/// A session ends this long after sign-in, however active it is.
pub const SESSION_TTL_HOURS: i32 = 12;
/// ...or after this long without a request.
pub const IDLE_TIMEOUT_MINS: i32 = 60;

pub const MIN_PASSWORD_LEN: usize = 12;

/// A forgotten-password link works this long, and only once.
pub const RESET_LINK_TTL_MINS: i32 = 60;
/// How long a new super user has to set their password.
pub const INVITATION_TTL_MINS: i32 = 3 * 24 * 60;

/// Forgotten-password requests from one address.
pub const RESET_REQUESTS_PER_IP: Rule = Rule {
    name: "reset-ip",
    limit: 10,
    window: Duration::from_secs(15 * 60),
};

/// Forgotten-password emails to one address, so nobody can flood an inbox.
pub const RESET_REQUESTS_PER_EMAIL: Rule = Rule {
    name: "reset-email",
    limit: 3,
    window: Duration::from_secs(60 * 60),
};

/// Failed sign-ins from one address before it has to wait.
pub const LOGIN_FAILURES_PER_IP: Rule = Rule {
    name: "login-ip",
    limit: 30,
    window: Duration::from_secs(15 * 60),
};

/// Failed sign-ins for one email before it has to wait, from any address.
pub const LOGIN_FAILURES_PER_EMAIL: Rule = Rule {
    name: "login-email",
    limit: 5,
    window: Duration::from_secs(15 * 60),
};

/// A master-tenant account as the super users' list shows it.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct UserSummary {
    pub id: Uuid,
    pub email: String,
    pub full_name: String,
    pub is_active: bool,
    /// `false` while an invited user hasn't set their password yet.
    pub has_password: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub tenant_id: TenantId,
    pub email: String,
    pub full_name: String,
    pub is_master: bool,
}

impl User {
    /// Only master-tenant users run the platform.
    pub fn require_master(&self) -> Result<(), AppError> {
        if self.is_master {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }
}

/// Emails are stored lowercase (the table enforces it), so every lookup
/// goes through here first.
///
/// ```
/// use admin::modules::identity::domain::normalize_email;
///
/// assert_eq!(normalize_email("  Jane@Example.COM "), "jane@example.com");
/// ```
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// ```
/// use admin::modules::identity::domain::check_new_password;
///
/// assert!(check_new_password("long enough password").is_ok());
/// assert!(check_new_password("short").is_err());
/// ```
pub fn check_new_password(password: &str) -> Result<(), AppError> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AppError::Invalid("password must be at least 12 characters"));
    }
    Ok(())
}
