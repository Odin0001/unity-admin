mod common;

use axum::http::StatusCode;
use common::{bearer, new_tenant, TestApp};
use serde_json::{json, Value};
use sqlx::PgPool;

async fn subscribe(app: &TestApp, auth: &str, tenant_id: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .send(
            "PUT",
            &format!("/api/v1/subscriptions/{tenant_id}"),
            &[("authorization", auth)],
            Some(body),
        )
        .await;
    (response.status, response.body)
}

#[sqlx::test]
async fn lists_the_seeded_plans(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);

    let plans = app
        .send("GET", "/api/v1/plans", &[("authorization", &auth)], None)
        .await;
    assert_eq!(plans.status, StatusCode::OK);
    let codes: Vec<&str> = plans
        .body
        .as_array()
        .expect("plans")
        .iter()
        .filter_map(|p| p["code"].as_str())
        .collect();
    assert_eq!(codes, ["starter", "growth", "enterprise"]);
}

#[sqlx::test]
async fn subscribing_publishes_the_plan_as_allowances(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (tenant_id, _) = new_tenant(&app, &auth, "acme").await;

    let (status, doc) = subscribe(
        &app,
        &auth,
        &tenant_id,
        json!({ "plan": "growth", "interval": "monthly" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["version"], 1);
    assert_eq!(doc["status"], "active");
    assert_eq!(doc["modules"], json!(["sales", "collection"]));
    assert_eq!(
        doc["limits"],
        json!({ "seats": 25, "leads": 10000, "cases": 5000, "esignatures_per_month": 300 })
    );
    assert_eq!(doc["calls"]["billing"], "prepaid");
    assert_eq!(doc["calls"]["balance_cents"], 0);
    assert_eq!(
        doc["transaction_fee"],
        json!({ "percent_bp": 150, "fixed_cents": 25 })
    );
    assert!(doc["valid_until"].is_string());

    // Changing plan bumps the version and moves to an annual period.
    let (_, doc) = subscribe(
        &app,
        &auth,
        &tenant_id,
        json!({ "plan": "enterprise", "interval": "annual" }),
    )
    .await;
    assert_eq!(doc["version"], 2);
    assert_eq!(doc["modules"], json!(["sales", "collection", "hr"]));
    let start = doc["period"]["start"].as_str().expect("start");
    let end = doc["period"]["end"].as_str().expect("end");
    assert_eq!(&start[4..], &end[4..], "annual period ends a year later");
}

#[sqlx::test]
async fn rejects_unknown_plans_and_the_master_tenant(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (tenant_id, _) = new_tenant(&app, &auth, "acme").await;

    let (status, _) = subscribe(
        &app,
        &auth,
        &tenant_id,
        json!({ "plan": "platinum", "interval": "monthly" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let master_id: String = sqlx::query_scalar("SELECT id::text FROM tenants WHERE is_master")
        .fetch_one(&app.db)
        .await
        .expect("master tenant");
    let (status, _) = subscribe(
        &app,
        &auth,
        &master_id,
        json!({ "plan": "growth", "interval": "monthly" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn tenant_fetches_its_own_allowances_with_its_key(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (tenant_id, api_key) = new_tenant(&app, &auth, "acme").await;
    let key = [("x-tenant-key", api_key.as_str())];

    let before_plan = app
        .send("GET", "/api/v1/internal/allowances", &key, None)
        .await;
    assert_eq!(before_plan.status, StatusCode::NOT_FOUND);

    subscribe(
        &app,
        &auth,
        &tenant_id,
        json!({ "plan": "starter", "interval": "monthly" }),
    )
    .await;
    sqlx::query(
        "INSERT INTO credit_ledger (tenant_id, amount_cents, reason)
         VALUES ($1::uuid, 50000, 'top-up'), ($1::uuid, -1200, 'calls')",
    )
    .bind(&tenant_id)
    .execute(&app.db)
    .await
    .expect("ledger");

    let doc = app
        .send("GET", "/api/v1/internal/allowances", &key, None)
        .await;
    assert_eq!(doc.status, StatusCode::OK);
    assert_eq!(doc.body["modules"], json!(["collection"]));
    assert_eq!(doc.body["calls"]["balance_cents"], 48800);

    let forged = app
        .send(
            "GET",
            "/api/v1/internal/allowances",
            &[("x-tenant-key", "nope")],
            None,
        )
        .await;
    assert_eq!(forged.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn suspension_shows_up_in_the_document(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (tenant_id, api_key) = new_tenant(&app, &auth, "acme").await;
    subscribe(
        &app,
        &auth,
        &tenant_id,
        json!({ "plan": "starter", "interval": "monthly" }),
    )
    .await;

    sqlx::query("UPDATE tenants SET status = 'suspended' WHERE id = $1::uuid")
        .bind(&tenant_id)
        .execute(&app.db)
        .await
        .expect("suspend");

    let doc = app
        .send(
            "GET",
            "/api/v1/internal/allowances",
            &[("x-tenant-key", &api_key)],
            None,
        )
        .await;
    assert_eq!(doc.body["status"], "suspended");
}
