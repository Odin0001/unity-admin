//! Rate limits on sign-in and the like, counted in Redis so every instance
//! shares them. When Redis can't be reached requests go through: locking
//! everyone out is worse than a short gap in brute-force protection.

use std::{
    convert::Infallible,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::request::Parts,
};
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use sha2::{Digest, Sha256};

use super::{error::AppError, AppState};

/// At most `limit` hits per `window` for one key.
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub name: &'static str,
    pub limit: u32,
    pub window: Duration,
}

#[derive(Clone)]
pub struct RateLimiter {
    redis: Option<ConnectionManager>,
    prefix: Arc<str>,
    trust_forwarded: bool,
}

impl RateLimiter {
    /// No limits at all. For tests and for running without Redis.
    pub fn disabled() -> Self {
        Self {
            redis: None,
            prefix: Arc::from(""),
            trust_forwarded: false,
        }
    }

    /// Fails if Redis can't be reached now; once running, Redis outages
    /// only switch the limits off until it's back. `prefix` keeps services
    /// sharing one Redis apart.
    pub async fn connect(url: &str, prefix: &str) -> Result<Self, redis::RedisError> {
        let config = ConnectionManagerConfig::new()
            .set_connection_timeout(Some(Duration::from_secs(2)))
            .set_response_timeout(Some(Duration::from_millis(500)));
        let client = redis::Client::open(url)?;
        let redis = ConnectionManager::new_with_config(client, config).await?;
        Ok(Self {
            redis: Some(redis),
            prefix: Arc::from(prefix),
            trust_forwarded: false,
        })
    }

    /// Reads `REDIS_URL` (unset or empty means no limits) and `TRUST_PROXY`
    /// (`true` behind a load balancer that appends the client's address to
    /// `X-Forwarded-For`).
    pub async fn from_env(prefix: &str) -> Result<Self, redis::RedisError> {
        let mut limiter = match std::env::var("REDIS_URL").ok().filter(|v| !v.is_empty()) {
            Some(url) => Self::connect(&url, prefix).await?,
            None => {
                tracing::warn!("REDIS_URL is not set; sign-in attempts are not rate limited");
                Self::disabled()
            }
        };
        limiter.trust_forwarded = std::env::var("TRUST_PROXY").is_ok_and(|v| v == "true");
        Ok(limiter)
    }

    /// Refuses once `key` has used up `rule`. Doesn't count anything itself.
    pub async fn check(&self, rule: Rule, key: &str) -> Result<(), AppError> {
        let Some(mut redis) = self.redis.clone() else {
            return Ok(());
        };
        let key = self.key(rule, key);
        let counted: Result<(Option<u32>, i64), _> = redis::pipe()
            .get(&key)
            .ttl(&key)
            .query_async(&mut redis)
            .await;

        match counted {
            Ok((Some(hits), ttl)) if hits >= rule.limit => Err(AppError::TooManyRequests {
                retry_after: ttl.max(1).unsigned_abs(),
            }),
            Ok(_) => Ok(()),
            Err(err) => {
                tracing::warn!(%err, "rate limit check skipped");
                Ok(())
            }
        }
    }

    /// For limits on every request rather than on failures: refuses once
    /// `key` is used up, and counts this request otherwise.
    pub async fn count(&self, rule: Rule, key: &str) -> Result<(), AppError> {
        self.check(rule, key).await?;
        self.hit(rule, key).await;
        Ok(())
    }

    /// Counts one hit against `key`. The window starts with the first hit.
    pub async fn hit(&self, rule: Rule, key: &str) {
        let Some(mut redis) = self.redis.clone() else {
            return;
        };
        let key = self.key(rule, key);
        let counted: Result<(), _> = redis::pipe()
            .atomic()
            .incr(&key, 1)
            .ignore()
            .cmd("EXPIRE")
            .arg(&key)
            .arg(rule.window.as_secs())
            .arg("NX")
            .ignore()
            .query_async(&mut redis)
            .await;
        if let Err(err) = counted {
            tracing::warn!(%err, "rate limit hit not counted");
        }
    }

    /// Hashed, so emails never sit in Redis in the clear.
    fn key(&self, rule: Rule, key: &str) -> String {
        let digest = Sha256::digest(key.as_bytes());
        format!(
            "ratelimit:{}:{}:{}",
            self.prefix,
            rule.name,
            hex::encode(&digest[..16])
        )
    }
}

/// The caller's address, for per-IP limits. Taken from the connection, or
/// from the last `X-Forwarded-For` entry (the one our load balancer added)
/// when `TRUST_PROXY` is on.
pub struct ClientIp(pub Option<IpAddr>);

impl ClientIp {
    pub fn key(&self) -> String {
        self.0
            .map_or_else(|| "unknown".to_string(), |ip| ip.to_string())
    }
}

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Infallible> {
        let forwarded = state
            .limiter
            .trust_forwarded
            .then(|| parts.headers.get("x-forwarded-for")?.to_str().ok())
            .flatten()
            .and_then(|list| list.rsplit(',').next()?.trim().parse().ok());
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        Ok(Self(forwarded.or(peer)))
    }
}
