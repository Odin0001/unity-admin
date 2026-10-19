use serde::Serialize;
use uuid::Uuid;

use crate::{platform::error::AppError, shared_kernel::TenantId};

/// A session ends this long after sign-in, however active it is.
pub const SESSION_TTL_HOURS: i32 = 12;
/// ...or after this long without a request.
pub const IDLE_TIMEOUT_MINS: i32 = 60;

pub const MIN_PASSWORD_LEN: usize = 12;

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
