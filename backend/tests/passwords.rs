//! Forgotten passwords, changing a password, and inviting super users.

mod common;

use axum::http::StatusCode;
use common::{bearer, redis_limiter, TestApp, PUBLIC_URL, SUPER_USER_EMAIL, SUPER_USER_PASSWORD};
use serde_json::{json, Value};
use sqlx::PgPool;

const NEW_PASSWORD: &str = "a-brand-new-password";

async fn post(app: &TestApp, uri: &str, auth: Option<&str>, body: Value) -> (StatusCode, Value) {
    let headers: Vec<(&str, &str)> = auth.map(|a| ("authorization", a)).into_iter().collect();
    let response = app.send("POST", uri, &headers, Some(body)).await;
    (response.status, response.body)
}

async fn me(app: &TestApp, token: &str) -> StatusCode {
    app.send(
        "GET",
        "/api/v1/auth/me",
        &[("authorization", &bearer(token))],
        None,
    )
    .await
    .status
}

#[sqlx::test]
async fn forgotten_password_link_sets_a_new_one(db: PgPool) {
    let app = TestApp::new(db).await;
    let old_session = app.super_user_token().await;

    // Unknown emails get the same answer, and no email goes out.
    let (status, _) = post(
        &app,
        "/api/v1/auth/password-reset",
        None,
        json!({ "email": "nobody@test.local" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(app.emails_to("nobody@test.local"), 0);

    let (status, _) = post(
        &app,
        "/api/v1/auth/password-reset",
        None,
        json!({ "email": " Super.User@Test.Local " }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let token = app
        .link_token(SUPER_USER_EMAIL)
        .expect("reset email with a link");
    let links_here = app.outbox.lock().expect("outbox")[0]
        .body
        .contains(&format!("{PUBLIC_URL}/reset-password?token="));
    assert!(links_here);

    let (status, _) = post(
        &app,
        "/api/v1/auth/password-reset/confirm",
        None,
        json!({ "token": token, "password": "short" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = post(
        &app,
        "/api/v1/auth/password-reset/confirm",
        None,
        json!({ "token": token, "password": NEW_PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(
        me(&app, &old_session).await,
        StatusCode::UNAUTHORIZED,
        "signed out everywhere"
    );
    assert_eq!(
        app.login(SUPER_USER_EMAIL, SUPER_USER_PASSWORD)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.login(SUPER_USER_EMAIL, NEW_PASSWORD).await.status,
        StatusCode::OK
    );

    let (status, body) = post(
        &app,
        "/api/v1/auth/password-reset/confirm",
        None,
        json!({ "token": token, "password": "yet-another-password" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "links work once");
    assert_eq!(body["error"], "this link is invalid or has expired");
}

#[sqlx::test]
async fn only_the_newest_unexpired_link_works(db: PgPool) {
    let app = TestApp::new(db).await;
    let reset = || {
        post(
            &app,
            "/api/v1/auth/password-reset",
            None,
            json!({ "email": SUPER_USER_EMAIL }),
        )
    };

    reset().await;
    let first = app.link_token(SUPER_USER_EMAIL).expect("first link");
    reset().await;
    let second = app.link_token(SUPER_USER_EMAIL).expect("second link");

    let confirm = |token: String| {
        post(
            &app,
            "/api/v1/auth/password-reset/confirm",
            None,
            json!({ "token": token, "password": NEW_PASSWORD }),
        )
    };
    assert_eq!(confirm(first).await.0, StatusCode::BAD_REQUEST);

    sqlx::query("UPDATE password_resets SET expires_at = now() - interval '1 minute'")
        .execute(&app.db)
        .await
        .expect("expire links");
    assert_eq!(confirm(second).await.0, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn changing_the_password_keeps_only_this_session(db: PgPool) {
    let app = TestApp::new(db).await;
    let this = app.super_user_token().await;
    let other = app.super_user_token().await;
    let auth = bearer(&this);
    let change = |current: &str, new: &str| {
        post(
            &app,
            "/api/v1/auth/password",
            Some(&auth),
            json!({ "current_password": current, "new_password": new }),
        )
    };

    assert_eq!(
        change("wrong-password", NEW_PASSWORD).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        change(SUPER_USER_PASSWORD, SUPER_USER_PASSWORD).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        change(SUPER_USER_PASSWORD, NEW_PASSWORD).await.0,
        StatusCode::NO_CONTENT
    );

    assert_eq!(me(&app, &this).await, StatusCode::OK);
    assert_eq!(me(&app, &other).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        app.login(SUPER_USER_EMAIL, NEW_PASSWORD).await.status,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn super_users_invite_super_users(db: PgPool) {
    let app = TestApp::new(db).await;
    let auth = bearer(&app.super_user_token().await);
    let invite = json!({ "email": "Second@Test.Local", "full_name": "Second Admin" });

    let (status, body) = post(&app, "/api/v1/users", Some(&auth), invite.clone()).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["email"], "second@test.local");
    assert_eq!(body["is_master"], true);
    assert_eq!(
        post(&app, "/api/v1/users", Some(&auth), invite).await.0,
        StatusCode::CONFLICT
    );

    let listed = app
        .send("GET", "/api/v1/users", &[("authorization", &auth)], None)
        .await;
    let has_password: Vec<_> = listed
        .body
        .as_array()
        .expect("list")
        .iter()
        .map(|u| u["has_password"].clone())
        .collect();
    assert_eq!(has_password, [json!(true), json!(false)]);

    // No password until they follow the invitation.
    let token = app
        .link_token("second@test.local")
        .expect("invitation email");
    assert_eq!(
        app.login("second@test.local", NEW_PASSWORD).await.status,
        StatusCode::UNAUTHORIZED
    );
    let (status, _) = post(
        &app,
        "/api/v1/auth/password-reset/confirm",
        None,
        json!({ "token": token, "password": NEW_PASSWORD }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let signed_in = app.login("second@test.local", NEW_PASSWORD).await;
    assert_eq!(signed_in.status, StatusCode::OK);
    let second = bearer(signed_in.body["token"].as_str().expect("token"));
    let tenants = app
        .send(
            "GET",
            "/api/v1/tenants",
            &[("authorization", &second)],
            None,
        )
        .await;
    assert_eq!(tenants.status, StatusCode::OK, "a full super user");
}

#[sqlx::test]
async fn reset_emails_to_one_address_are_limited(db: PgPool) {
    let app = TestApp::with_limiter(db, redis_limiter().await).await;
    for _ in 0..3 {
        let (status, _) = post(
            &app,
            "/api/v1/auth/password-reset",
            None,
            json!({ "email": SUPER_USER_EMAIL }),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }
    let (status, _) = post(
        &app,
        "/api/v1/auth/password-reset",
        None,
        json!({ "email": SUPER_USER_EMAIL }),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(app.emails_to(SUPER_USER_EMAIL), 3);
}
