mod common;

use admin::platform::{password, token};
use axum::http::StatusCode;
use common::{bearer, TestApp};
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn super_user_creates_a_tenant_and_sees_its_key_once(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);

    let created = app
        .send(
            "POST",
            "/api/v1/tenants",
            &[("authorization", &auth)],
            Some(json!({ "slug": "acme", "name": "Acme Collections" })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.body["tenant"]["slug"], "acme");
    assert_eq!(created.body["tenant"]["status"], "active");
    assert_eq!(created.body["api_key"].as_str().map(str::len), Some(43));

    let listed = app
        .send("GET", "/api/v1/tenants", &[("authorization", &auth)], None)
        .await;
    assert_eq!(listed.status, StatusCode::OK);
    let tenants = listed.body.as_array().expect("tenant list");
    assert_eq!(tenants.len(), 2, "master tenant + acme");
    assert!(tenants.iter().all(|t| t.get("api_key").is_none()));
}

#[sqlx::test]
async fn rejects_invalid_and_duplicate_slugs(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let headers = [("authorization", auth.as_str())];
    let create = |slug: &'static str| {
        app.send(
            "POST",
            "/api/v1/tenants",
            &headers,
            Some(json!({ "slug": slug, "name": "Acme" })),
        )
    };

    assert_eq!(create("Not_A_Slug").await.status, StatusCode::BAD_REQUEST);
    assert_eq!(create("acme").await.status, StatusCode::CREATED);
    assert_eq!(create("acme").await.status, StatusCode::CONFLICT);
    assert_eq!(create("platform").await.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn users_outside_the_master_tenant_cannot_manage_tenants(db: PgPool) {
    let app = TestApp::new(db).await;

    sqlx::query("INSERT INTO tenants (slug, name, pin, api_key_hash) VALUES ('acme', 'Acme', 'ACME42', 'x')")
        .execute(&app.db)
        .await
        .expect("create tenant");
    let password_hash = password::hash("acme-user-password".into())
        .await
        .expect("hash password");
    sqlx::query(
        "INSERT INTO users (tenant_id, email, password_hash, full_name)
         SELECT id, 'agent@acme.test', $1, 'Acme Agent' FROM tenants WHERE slug = 'acme'",
    )
    .bind(password_hash)
    .execute(&app.db)
    .await
    .expect("create tenant user");

    let login = app.login("agent@acme.test", "acme-user-password").await;
    assert_eq!(login.status, StatusCode::OK);
    assert_eq!(login.body["user"]["is_master"], false);
    let auth = bearer(login.body["token"].as_str().expect("token"));

    let listed = app
        .send("GET", "/api/v1/tenants", &[("authorization", &auth)], None)
        .await;
    assert_eq!(listed.status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn tenant_api_key_authenticates_internal_calls(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let created = app
        .send(
            "POST",
            "/api/v1/tenants",
            &[("authorization", &auth)],
            Some(json!({ "slug": "acme", "name": "Acme Collections" })),
        )
        .await;
    let api_key = created.body["api_key"].as_str().expect("api key");

    let missing = app.send("GET", "/api/v1/internal/tenant", &[], None).await;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);

    let forged = app
        .send(
            "GET",
            "/api/v1/internal/tenant",
            &[("x-tenant-key", &token::generate())],
            None,
        )
        .await;
    assert_eq!(forged.status, StatusCode::UNAUTHORIZED);

    // A user session token is not a tenant key.
    let session_as_key = app
        .send(
            "GET",
            "/api/v1/internal/tenant",
            &[("x-tenant-key", auth.trim_start_matches("Bearer "))],
            None,
        )
        .await;
    assert_eq!(session_as_key.status, StatusCode::UNAUTHORIZED);

    let me = app
        .send(
            "GET",
            "/api/v1/internal/tenant",
            &[("x-tenant-key", api_key)],
            None,
        )
        .await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["slug"], "acme");
    assert!(me.body["last_seen_at"].is_string(), "check-in is recorded");
}

#[sqlx::test]
async fn unknown_api_routes_are_json_404s(db: PgPool) {
    let app = TestApp::new(db).await;
    let response = app.send("GET", "/api/v1/nope", &[], None).await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(response.body["error"], "not found");
}
