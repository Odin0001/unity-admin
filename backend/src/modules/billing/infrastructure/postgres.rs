use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;

use crate::{
    modules::{
        billing::domain::{
            Allowances, BillingInterval, CallBilling, Calls, Limits, Module, Period, Plan,
            TransactionFee,
        },
        tenants::domain::TenantStatus,
    },
    shared_kernel::TenantId,
};

pub async fn plans(db: &PgPool) -> Result<Vec<Plan>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, code, name, modules, seats, leads, cases, esignatures_per_month,
                call_billing, price_per_minute_cents, transaction_fee_bp,
                transaction_fee_fixed_cents, monthly_price_cents, annual_price_cents
         FROM plans
         ORDER BY monthly_price_cents",
    )
    .fetch_all(db)
    .await
}

/// Puts the tenant on `plan_code` with a fresh period starting today.
/// Returns `false` if there's no such plan.
pub async fn subscribe(
    db: &PgPool,
    tenant_id: TenantId,
    plan_code: &str,
    interval: BillingInterval,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO subscriptions
             (tenant_id, plan_id, billing_interval, current_period_start, current_period_end)
         SELECT $1, p.id, $3, current_date,
                (current_date + CASE WHEN $3 = 'annual' THEN interval '1 year'
                                     ELSE interval '1 month' END)::date
         FROM plans p
         WHERE p.code = $2
         ON CONFLICT (tenant_id) DO UPDATE
         SET plan_id = EXCLUDED.plan_id,
             billing_interval = EXCLUDED.billing_interval,
             current_period_start = EXCLUDED.current_period_start,
             current_period_end = EXCLUDED.current_period_end,
             version = subscriptions.version + 1,
             updated_at = now()",
    )
    .bind(tenant_id)
    .bind(plan_code)
    .bind(interval)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

#[derive(sqlx::FromRow)]
struct AllowancesRow {
    status: TenantStatus,
    pin: String,
    version: i64,
    current_period_start: NaiveDate,
    current_period_end: NaiveDate,
    modules: Vec<Module>,
    seats: i32,
    leads: i32,
    cases: i32,
    esignatures_per_month: i32,
    call_billing: CallBilling,
    price_per_minute_cents: i32,
    transaction_fee_bp: i32,
    transaction_fee_fixed_cents: i32,
    call_balance_cents: i64,
}

/// `None` when the tenant has no subscription.
pub async fn allowances(
    db: &PgPool,
    tenant_id: TenantId,
    valid_until: DateTime<Utc>,
) -> Result<Option<Allowances>, sqlx::Error> {
    let row: Option<AllowancesRow> = sqlx::query_as(
        "SELECT t.status, t.pin, s.version, s.current_period_start, s.current_period_end,
                p.modules, p.seats, p.leads, p.cases, p.esignatures_per_month,
                p.call_billing, p.price_per_minute_cents, p.transaction_fee_bp,
                p.transaction_fee_fixed_cents,
                COALESCE((SELECT sum(c.amount_cents) FROM credit_ledger c
                          WHERE c.tenant_id = t.id), 0)::bigint AS call_balance_cents
         FROM tenants t
         JOIN subscriptions s ON s.tenant_id = t.id
         JOIN plans p ON p.id = s.plan_id
         WHERE t.id = $1",
    )
    .bind(tenant_id)
    .fetch_optional(db)
    .await?;

    Ok(row.map(|r| Allowances {
        version: r.version,
        status: r.status,
        pin: r.pin,
        modules: r.modules,
        limits: Limits {
            seats: r.seats,
            leads: r.leads,
            cases: r.cases,
            esignatures_per_month: r.esignatures_per_month,
        },
        calls: Calls {
            billing: r.call_billing,
            balance_cents: r.call_balance_cents,
            price_per_minute_cents: r.price_per_minute_cents,
        },
        transaction_fee: TransactionFee {
            percent_bp: r.transaction_fee_bp,
            fixed_cents: r.transaction_fee_fixed_cents,
        },
        period: Period {
            start: r.current_period_start,
            end: r.current_period_end,
        },
        valid_until,
    }))
}
