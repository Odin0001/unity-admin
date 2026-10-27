use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{modules::tenants::domain::TenantStatus, shared_kernel::TenantId};

/// Deployments report every minute; three missed beats and a tenant counts
/// as quiet.
pub const QUIET_AFTER: Duration = Duration::minutes(3);

/// What a tenant deployment reports about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub version: String,
    pub last_migration: Option<i64>,
    pub active_employees: i64,
    pub active_sessions: i64,
    pub database_bytes: i64,
}

#[derive(Debug, Serialize)]
pub struct TenantHealth {
    pub tenant_id: TenantId,
    pub slug: String,
    pub name: String,
    pub status: TenantStatus,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub heartbeat: Option<Heartbeat>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub quiet: bool,
}

/// A tenant that never sent a heartbeat is quiet too.
///
/// ```
/// use admin::modules::monitoring::domain::is_quiet;
/// use chrono::{Duration, Utc};
///
/// let now = Utc::now();
/// assert!(!is_quiet(Some(now - Duration::seconds(90)), now));
/// assert!(is_quiet(Some(now - Duration::minutes(5)), now));
/// assert!(is_quiet(None, now));
/// ```
pub fn is_quiet(heartbeat_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    heartbeat_at.is_none_or(|at| now - at > QUIET_AFTER)
}
