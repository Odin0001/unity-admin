//! Admin controlling tenant deployments: status changes, and the signed
//! call that makes a deployment pick them up at once.

mod common;

use std::time::Duration;

use admin::modules::tenants::signed_message;
use axum::{http::HeaderMap, routing::post, Router};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use common::{bearer, new_tenant, TestApp, SIGNING_SEED};
use ed25519_dalek::{Signature, SigningKey};
use serde_json::json;
use sqlx::PgPool;
use tokio::{net::TcpListener, sync::mpsc};

const REFRESH_PATH: &str = "/api/v1/internal/admin/refresh-allowances";

/// A stand-in tenant deployment that forwards the headers of every refresh
/// call it gets. Returns its origin.
async fn fake_tenant() -> (String, mpsc::UnboundedReceiver<HeaderMap>) {
    let (calls, received) = mpsc::unbounded_channel();
    let router = Router::new().route(
        REFRESH_PATH,
        post(move |headers: HeaderMap| async move {
            let _ = calls.send(headers);
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    tokio::spawn(async move { axum::serve(listener, router).await });
    (format!("http://{addr}"), received)
}

#[sqlx::test]
async fn status_changes_are_published_with_a_new_version(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (id, key) = new_tenant(&app, &auth, "acme").await;
    app.send(
        "PUT",
        &format!("/api/v1/subscriptions/{id}"),
        &[("authorization", &auth)],
        Some(json!({ "plan": "growth", "interval": "monthly" })),
    )
    .await;

    let changed = app
        .send(
            "PUT",
            &format!("/api/v1/tenants/{id}/status"),
            &[("authorization", &auth)],
            Some(json!({ "status": "past_due" })),
        )
        .await;
    assert_eq!(changed.status, 200, "{}", changed.body);
    assert_eq!(changed.body["status"], "past_due");

    let document = app
        .send(
            "GET",
            "/api/v1/internal/allowances",
            &[("x-tenant-key", &key)],
            None,
        )
        .await;
    assert_eq!(document.body["status"], "past_due");
    assert_eq!(document.body["version"], 2);
}

#[sqlx::test]
async fn status_rules(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (id, _) = new_tenant(&app, &auth, "acme").await;
    let master: String = sqlx::query_scalar("SELECT id::text FROM tenants WHERE is_master")
        .fetch_one(&app.db)
        .await
        .expect("master tenant");

    let set = |id: String, status: &'static str| {
        let auth = auth.clone();
        let app = &app;
        async move {
            app.send(
                "PUT",
                &format!("/api/v1/tenants/{id}/status"),
                &[("authorization", &auth)],
                Some(json!({ "status": status })),
            )
            .await
            .status
        }
    };

    assert_eq!(set(master, "suspended").await, 400);
    assert_eq!(set(id.clone(), "paused").await, 422);
    assert_eq!(
        set(uuid::Uuid::new_v4().to_string(), "suspended").await,
        404
    );
    assert_eq!(set(id, "cancelled").await, 200);
}

#[sqlx::test]
async fn api_url_must_be_http(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (id, _) = new_tenant(&app, &auth, "acme").await;
    let uri = format!("/api/v1/tenants/{id}");

    let bad = app
        .send(
            "PATCH",
            &uri,
            &[("authorization", &auth)],
            Some(json!({ "api_url": "acme.example.com" })),
        )
        .await;
    assert_eq!(bad.status, 400);

    let good = app
        .send(
            "PATCH",
            &uri,
            &[("authorization", &auth)],
            Some(json!({ "api_url": "https://acme.example.com/" })),
        )
        .await;
    assert_eq!(good.status, 200);
    assert_eq!(good.body["api_url"], "https://acme.example.com");
    assert_eq!(good.body["name"], "acme");
}

#[sqlx::test]
async fn status_change_sends_the_tenant_a_signed_refresh(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let (id, _) = new_tenant(&app, &auth, "acme").await;
    let (api_url, mut calls) = fake_tenant().await;
    app.send(
        "PATCH",
        &format!("/api/v1/tenants/{id}"),
        &[("authorization", &auth)],
        Some(json!({ "api_url": api_url })),
    )
    .await;

    app.send(
        "PUT",
        &format!("/api/v1/tenants/{id}/status"),
        &[("authorization", &auth)],
        Some(json!({ "status": "suspended" })),
    )
    .await;

    let headers = tokio::time::timeout(Duration::from_secs(5), calls.recv())
        .await
        .expect("tenant was called")
        .expect("call headers");
    let header = |name| headers[name].to_str().expect("ascii header");
    let timestamp: i64 = header("x-admin-timestamp").parse().expect("unix seconds");
    assert!((Utc::now().timestamp() - timestamp).abs() < 60);

    let signature = URL_SAFE_NO_PAD
        .decode(header("x-admin-signature"))
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .expect("signature");
    let public_key = SigningKey::from_bytes(&SIGNING_SEED).verifying_key();
    let message = signed_message("POST", REFRESH_PATH, timestamp, &[]);
    assert!(public_key.verify_strict(&message, &signature).is_ok());
}
