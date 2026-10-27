use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::modules::tenants::domain::TenantStatus;

/// How long a tenant may keep enforcing an allowances document without
/// hearing from admin again.
pub const ALLOWANCES_TTL_HOURS: i64 = 72;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Module {
    Sales,
    Collection,
    Hr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum BillingInterval {
    Monthly,
    Annual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum CallBilling {
    Prepaid,
    Postpaid,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Plan {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub modules: Vec<Module>,
    pub seats: i32,
    pub leads: i32,
    pub cases: i32,
    pub esignatures_per_month: i32,
    pub call_billing: CallBilling,
    pub price_per_minute_cents: i32,
    pub transaction_fee_bp: i32,
    pub transaction_fee_fixed_cents: i32,
    pub monthly_price_cents: i64,
    pub annual_price_cents: i64,
}

/// What a tenant is allowed, as published to it. The shape is a contract
/// with the tenant service (see `references/architecture.md`, section 3).
#[derive(Debug, Serialize)]
pub struct Allowances {
    pub version: i64,
    pub status: TenantStatus,
    /// The tenant checks it on every sign-in.
    pub pin: String,
    pub modules: Vec<Module>,
    pub limits: Limits,
    pub calls: Calls,
    pub transaction_fee: TransactionFee,
    pub period: Period,
    pub valid_until: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct Limits {
    pub seats: i32,
    pub leads: i32,
    pub cases: i32,
    pub esignatures_per_month: i32,
}

#[derive(Debug, Serialize)]
pub struct Calls {
    pub billing: CallBilling,
    pub balance_cents: i64,
    pub price_per_minute_cents: i32,
}

#[derive(Debug, Serialize)]
pub struct TransactionFee {
    /// Basis points: 150 is 1.5%.
    pub percent_bp: i32,
    pub fixed_cents: i32,
}

#[derive(Debug, Serialize)]
pub struct Period {
    pub start: NaiveDate,
    pub end: NaiveDate,
}
