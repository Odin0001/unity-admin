use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::{
    modules::{
        monitoring::domain::{is_quiet, Heartbeat, TenantHealth},
        tenants::domain::TenantStatus,
    },
    shared_kernel::TenantId,
};

/// Only the latest heartbeat per tenant is kept.
pub async fn record(db: &PgPool, tenant_id: TenantId, beat: &Heartbeat) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tenant_heartbeats
             (tenant_id, version, last_migration, active_employees, active_sessions, database_bytes)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (tenant_id) DO UPDATE SET
             version = EXCLUDED.version,
             last_migration = EXCLUDED.last_migration,
             active_employees = EXCLUDED.active_employees,
             active_sessions = EXCLUDED.active_sessions,
             database_bytes = EXCLUDED.database_bytes,
             received_at = now()",
    )
    .bind(tenant_id)
    .bind(&beat.version)
    .bind(beat.last_migration)
    .bind(beat.active_employees)
    .bind(beat.active_sessions)
    .bind(beat.database_bytes)
    .execute(db)
    .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct Row {
    id: TenantId,
    slug: String,
    name: String,
    status: TenantStatus,
    last_seen_at: Option<DateTime<Utc>>,
    version: Option<String>,
    last_migration: Option<i64>,
    active_employees: Option<i64>,
    active_sessions: Option<i64>,
    database_bytes: Option<i64>,
    received_at: Option<DateTime<Utc>>,
}

/// Every customer tenant, quiet ones first.
pub async fn overview(db: &PgPool, now: DateTime<Utc>) -> Result<Vec<TenantHealth>, sqlx::Error> {
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT t.id, t.slug, t.name, t.status, t.last_seen_at,
                h.version, h.last_migration, h.active_employees, h.active_sessions,
                h.database_bytes, h.received_at
         FROM tenants t
         LEFT JOIN tenant_heartbeats h ON h.tenant_id = t.id
         WHERE NOT t.is_master
         ORDER BY h.received_at ASC NULLS FIRST, t.slug",
    )
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| TenantHealth {
            tenant_id: r.id,
            slug: r.slug,
            name: r.name,
            status: r.status,
            last_seen_at: r.last_seen_at,
            heartbeat: r.version.map(|version| Heartbeat {
                version,
                last_migration: r.last_migration,
                active_employees: r.active_employees.unwrap_or_default(),
                active_sessions: r.active_sessions.unwrap_or_default(),
                database_bytes: r.database_bytes.unwrap_or_default(),
            }),
            heartbeat_at: r.received_at,
            quiet: is_quiet(r.received_at, now),
        })
        .collect())
}
