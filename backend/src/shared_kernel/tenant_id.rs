use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Its own type so a user id can't be passed where a tenant id belongs.
/// Stored as a Postgres `uuid`, serialized as a plain UUID string.
///
/// ```
/// use admin::shared_kernel::TenantId;
///
/// let id: TenantId = "befefd82-4ccb-418f-b3fe-d4795dbc96b9".parse()?;
/// assert_eq!(id.to_string(), "befefd82-4ccb-418f-b3fe-d4795dbc96b9");
/// # Ok::<(), uuid::Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct TenantId(Uuid);

impl From<Uuid> for TenantId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl FromStr for TenantId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_json_as_a_plain_uuid() -> Result<(), serde_json::Error> {
        let id = TenantId::from(Uuid::new_v4());
        let json = serde_json::to_string(&id)?;
        assert_eq!(json, format!("\"{id}\""));
        assert_eq!(serde_json::from_str::<TenantId>(&json)?, id);
        Ok(())
    }

    #[test]
    fn rejects_non_uuid_strings() {
        assert!("acme".parse::<TenantId>().is_err());
    }
}
