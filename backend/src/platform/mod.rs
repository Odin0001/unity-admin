pub mod db;
pub mod error;
pub mod mail;
pub mod password;
pub mod rate_limit;
pub mod token;

use sqlx::PgPool;

use crate::modules::tenants::TenantClient;
use mail::Mailer;
use rate_limit::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub tenant_client: TenantClient,
    pub limiter: RateLimiter,
    pub mailer: Mailer,
    /// Where people open this app, e.g. `https://admin.example.com`. Links
    /// in emails point here.
    pub public_url: String,
}
