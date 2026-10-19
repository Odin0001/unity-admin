use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::shared_kernel::TenantId;

/// Suspended tenants keep their data but their users can't sign in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum TenantStatus {
    Active,
    Suspended,
}

/// The API key hash is deliberately not part of this.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Tenant {
    pub id: TenantId,
    pub slug: String,
    pub name: String,
    pub status: TenantStatus,
    pub is_master: bool,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Lowercase letters, digits and dashes, 2-63 chars, not starting with a
/// dash. Mirrors the CHECK constraint on `tenants.slug`.
///
/// ```
/// use admin::modules::tenants::domain::is_valid_slug;
///
/// assert!(is_valid_slug("acme-collections"));
/// assert!(!is_valid_slug("Acme Collections"));
/// ```
pub fn is_valid_slug(slug: &str) -> bool {
    (2..=63).contains(&slug.len())
        && !slug.starts_with('-')
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::is_valid_slug;

    #[test]
    fn accepts_dns_style_slugs() {
        assert!(is_valid_slug("acme"));
        assert!(is_valid_slug("acme-collections-2"));
    }

    #[test]
    fn rejects_bad_slugs() {
        assert!(!is_valid_slug("a"));
        assert!(!is_valid_slug("-acme"));
        assert!(!is_valid_slug("Acme"));
        assert!(!is_valid_slug("acme_co"));
        assert!(!is_valid_slug(&"a".repeat(64)));
    }
}
