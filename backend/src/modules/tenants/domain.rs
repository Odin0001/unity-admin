use std::time::Duration;

use chrono::{DateTime, Utc};
use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};

use crate::{platform::rate_limit::Rule, shared_kernel::TenantId};

/// Characters a PIN is made of: no 0/O or 1/I to mix up.
const PIN_ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const PIN_LEN: usize = 6;

/// PIN lookups from one address. Low enough that guessing PINs to find
/// out who our customers are takes forever.
pub const LOOKUPS_PER_IP: Rule = Rule {
    name: "pin-lookup-ip",
    limit: 10,
    window: Duration::from_secs(60),
};

/// Past-due tenants keep working through the grace period. Suspended and
/// cancelled ones keep their data, but nobody can sign in or change
/// anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TenantStatus {
    Active,
    PastDue,
    Suspended,
    Cancelled,
}

impl TenantStatus {
    /// Whether the tenant's staff can work: active, or past due during the
    /// grace period.
    pub fn is_live(self) -> bool {
        matches!(self, Self::Active | Self::PastDue)
    }
}

/// The API key hash is deliberately not part of this.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Tenant {
    pub id: TenantId,
    pub slug: String,
    pub name: String,
    pub status: TenantStatus,
    pub is_master: bool,
    /// `None` only for the master tenant.
    pub pin: Option<String>,
    /// Where the deployment is reached; `None` until it's deployed.
    pub api_url: Option<String>,
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

/// ```
/// use admin::modules::tenants::domain::{generate_pin, normalize_pin};
///
/// let pin = generate_pin();
/// assert_eq!(normalize_pin(&pin.to_lowercase()), Some(pin));
/// ```
pub fn generate_pin() -> String {
    let mut rng = rand::rng();
    (0..PIN_LEN)
        .filter_map(|_| PIN_ALPHABET.choose(&mut rng).map(|&b| char::from(b)))
        .collect()
}

/// What someone typed, uppercased and trimmed, or `None` if it can't be a
/// PIN at all.
pub fn normalize_pin(input: &str) -> Option<String> {
    let pin = input.trim().to_ascii_uppercase();
    (pin.len() == PIN_LEN && pin.bytes().all(|b| PIN_ALPHABET.contains(&b))).then_some(pin)
}

/// An http(s) origin, normalised without the trailing slash.
///
/// ```
/// use admin::modules::tenants::domain::normalize_api_url;
///
/// assert_eq!(normalize_api_url(" https://acme.example.com/ ").as_deref(), Some("https://acme.example.com"));
/// assert_eq!(normalize_api_url("acme.example.com"), None);
/// ```
pub fn normalize_api_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let host = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    (!host.is_empty() && !host.contains(char::is_whitespace)).then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::{generate_pin, is_valid_slug, normalize_pin};

    #[test]
    fn accepts_dns_style_slugs() {
        assert!(is_valid_slug("acme"));
        assert!(is_valid_slug("acme-collections-2"));
    }

    #[test]
    fn pins_avoid_look_alike_characters() {
        assert_eq!(normalize_pin(" k7q2m9 ").as_deref(), Some("K7Q2M9"));
        assert_eq!(normalize_pin("K7Q2M0"), None);
        assert_eq!(normalize_pin("K7Q2MO"), None);
        assert_eq!(normalize_pin("K7Q2M"), None);
        for _ in 0..100 {
            let pin = generate_pin();
            assert_eq!(normalize_pin(&pin), Some(pin));
        }
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
