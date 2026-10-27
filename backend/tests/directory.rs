//! Tenant PINs and the shared login page's lookup.

mod common;

use axum::http::StatusCode;
use common::{bearer, new_tenant, redis_limiter, TestApp, SUPER_USER_EMAIL};
use serde_json::{json, Value};
use sqlx::PgPool;

async fn lookup(app: &TestApp, pin: &str) -> (StatusCode, Value) {
    let response = app
        .send(
            "POST",
            "/api/v1/directory/lookup",
            &[],
            Some(json!({ "pin": pin })),
        )
        .await;
    (response.status, response.body)
}

async fn tenant(app: &TestApp, auth: &str, id: &str) -> Value {
    let all = app
        .send("GET", "/api/v1/tenants", &[("authorization", auth)], None)
        .await;
    all.body
        .as_array()
        .into_iter()
        .flatten()
        .find(|t| t["id"] == id)
        .cloned()
        .expect("tenant in list")
}

#[sqlx::test]
async fn the_pin_leads_to_the_tenants_own_sign_in_page(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (id, _) = new_tenant(&app, &auth, "acme").await;
    let pin = tenant(&app, &auth, &id).await["pin"]
        .as_str()
        .expect("every customer tenant has a PIN")
        .to_string();
    assert_eq!(pin.len(), 6);

    // Not deployed yet: nowhere to send anyone.
    assert_eq!(lookup(&app, &pin).await.0, StatusCode::NOT_FOUND);

    app.send(
        "PATCH",
        &format!("/api/v1/tenants/{id}"),
        &[("authorization", &auth)],
        Some(json!({ "api_url": "https://acme.example.com" })),
    )
    .await;
    let (status, body) = lookup(&app, &format!(" {} ", pin.to_lowercase())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({ "name": "acme", "login_url": format!("https://acme.example.com/login?pin={pin}") })
    );

    for (status, expected) in [
        ("past_due", StatusCode::OK),
        ("suspended", StatusCode::NOT_FOUND),
    ] {
        app.send(
            "PUT",
            &format!("/api/v1/tenants/{id}/status"),
            &[("authorization", &auth)],
            Some(json!({ "status": status })),
        )
        .await;
        assert_eq!(lookup(&app, &pin).await.0, expected, "{status}");
    }
}

#[sqlx::test]
async fn unknown_and_malformed_pins_are_not_found(db: PgPool) {
    let app = TestApp::new(db).await;
    for pin in ["ZZZZZZ", "", "0O1I00", "acme"] {
        assert_eq!(lookup(&app, pin).await.0, StatusCode::NOT_FOUND, "{pin}");
    }
}

#[sqlx::test]
async fn rotating_the_pin_retires_the_old_one(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (id, key) = new_tenant(&app, &auth, "acme").await;
    app.send(
        "PATCH",
        &format!("/api/v1/tenants/{id}"),
        &[("authorization", &auth)],
        Some(json!({ "api_url": "https://acme.example.com" })),
    )
    .await;
    app.send(
        "PUT",
        &format!("/api/v1/subscriptions/{id}"),
        &[("authorization", &auth)],
        Some(json!({ "plan": "starter", "interval": "monthly" })),
    )
    .await;
    let old_pin = tenant(&app, &auth, &id).await["pin"].clone();

    let rotated = app
        .send(
            "POST",
            &format!("/api/v1/tenants/{id}/pin"),
            &[("authorization", &auth)],
            None,
        )
        .await;
    assert_eq!(rotated.status, StatusCode::OK);
    let new_pin = rotated.body["pin"].as_str().expect("new pin");
    assert_ne!(rotated.body["pin"], old_pin);

    assert_eq!(
        lookup(&app, old_pin.as_str().expect("pin")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(lookup(&app, new_pin).await.0, StatusCode::OK);

    // The tenant learns its new PIN from its allowances.
    let document = app
        .send(
            "GET",
            "/api/v1/internal/allowances",
            &[("x-tenant-key", &key)],
            None,
        )
        .await;
    assert_eq!(document.body["pin"], new_pin);
    assert_eq!(document.body["version"], 2);

    let master: String = sqlx::query_scalar("SELECT id::text FROM tenants WHERE is_master")
        .fetch_one(&app.db)
        .await
        .expect("master tenant");
    let refused = app
        .send(
            "POST",
            &format!("/api/v1/tenants/{master}/pin"),
            &[("authorization", &auth)],
            None,
        )
        .await;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn pin_lookups_are_rate_limited_per_address(db: PgPool) {
    let app = TestApp::with_limiter(db, redis_limiter().await).await;

    for _ in 0..10 {
        assert_eq!(lookup(&app, "ZZZZZZ").await.0, StatusCode::NOT_FOUND);
    }
    let (status, body) = lookup(&app, "ZZZZZZ").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(body["error"]
        .as_str()
        .is_some_and(|e| e.contains("try again")));
}

#[sqlx::test]
async fn repeated_wrong_passwords_lock_the_email_for_a_while(db: PgPool) {
    let app = TestApp::with_limiter(db, redis_limiter().await).await;

    for _ in 0..5 {
        let attempt = app.login(SUPER_USER_EMAIL, "wrong-password").await;
        assert_eq!(attempt.status, StatusCode::UNAUTHORIZED);
    }
    // Even the right password waits now.
    let locked = app
        .login(SUPER_USER_EMAIL, common::SUPER_USER_PASSWORD)
        .await;
    assert_eq!(locked.status, StatusCode::TOO_MANY_REQUESTS);
}

#[sqlx::test]
async fn successful_sign_ins_do_not_count(db: PgPool) {
    let app = TestApp::with_limiter(db, redis_limiter().await).await;
    for _ in 0..8 {
        app.super_user_token().await;
    }
}
