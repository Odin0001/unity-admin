mod common;

use common::{bearer, new_tenant, TestApp};
use serde_json::json;
use sqlx::PgPool;

fn heartbeat(version: &str) -> serde_json::Value {
    json!({
        "version": version,
        "last_migration": 20261026000001_i64,
        "active_employees": 4,
        "active_sessions": 2,
        "database_bytes": 9_000_000,
    })
}

#[sqlx::test]
async fn heartbeats_show_up_in_the_overview(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (acme, key) = new_tenant(&app, &auth, "acme").await;
    new_tenant(&app, &auth, "globex").await;

    let sent = app
        .send(
            "POST",
            "/api/v1/internal/heartbeat",
            &[("x-tenant-key", &key)],
            Some(heartbeat("0.1.0")),
        )
        .await;
    assert_eq!(sent.status, 204);

    let overview = app
        .send(
            "GET",
            "/api/v1/monitoring",
            &[("authorization", &auth)],
            None,
        )
        .await;
    assert_eq!(overview.status, 200);
    let tenants = overview.body.as_array().expect("list");
    assert_eq!(tenants.len(), 2, "the master tenant isn't monitored");

    // Quiet tenants come first.
    assert_eq!(tenants[0]["slug"], "globex");
    assert_eq!(tenants[0]["quiet"], true);
    assert_eq!(tenants[0]["heartbeat"], json!(null));

    assert_eq!(tenants[1]["tenant_id"], acme);
    assert_eq!(tenants[1]["quiet"], false);
    assert_eq!(tenants[1]["heartbeat"], heartbeat("0.1.0"));
}

#[sqlx::test]
async fn only_the_latest_heartbeat_is_kept(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (_, key) = new_tenant(&app, &auth, "acme").await;

    for version in ["0.1.0", "0.2.0"] {
        app.send(
            "POST",
            "/api/v1/internal/heartbeat",
            &[("x-tenant-key", &key)],
            Some(heartbeat(version)),
        )
        .await;
    }

    let overview = app
        .send(
            "GET",
            "/api/v1/monitoring",
            &[("authorization", &auth)],
            None,
        )
        .await;
    assert_eq!(overview.body[0]["heartbeat"]["version"], "0.2.0");
}

#[sqlx::test]
async fn heartbeats_need_a_tenant_key(db: PgPool) {
    let app = TestApp::new(db).await;

    let anonymous = app
        .send(
            "POST",
            "/api/v1/internal/heartbeat",
            &[],
            Some(heartbeat("0.1.0")),
        )
        .await;
    assert_eq!(anonymous.status, 401);

    let monitoring = app.send("GET", "/api/v1/monitoring", &[], None).await;
    assert_eq!(monitoring.status, 401);
}
