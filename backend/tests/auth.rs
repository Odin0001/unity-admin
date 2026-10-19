mod common;

use admin::{modules::identity::domain::IDLE_TIMEOUT_MINS, platform::token};
use axum::http::StatusCode;
use common::{bearer, TestApp, SUPER_USER_EMAIL, SUPER_USER_PASSWORD};
use sqlx::PgPool;

#[sqlx::test]
async fn login_issues_a_256_bit_token_and_stores_only_its_hash(db: PgPool) {
    let app = TestApp::new(db).await;

    let response = app.login(SUPER_USER_EMAIL, SUPER_USER_PASSWORD).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body["user"]["email"], SUPER_USER_EMAIL);
    assert_eq!(response.body["user"]["is_master"], true);

    let token = response.body["token"].as_str().expect("token");
    assert_eq!(token.len(), 43);

    let stored: Vec<String> = sqlx::query_scalar("SELECT token_hash FROM auth_sessions")
        .fetch_all(&app.db)
        .await
        .expect("query sessions");
    assert_eq!(stored, vec![token::hash(token)]);
}

#[sqlx::test]
async fn email_match_ignores_case_and_surrounding_whitespace(db: PgPool) {
    let app = TestApp::new(db).await;
    let response = app
        .login("  Super.User@Test.LOCAL ", SUPER_USER_PASSWORD)
        .await;
    assert_eq!(response.status, StatusCode::OK);
}

#[sqlx::test]
async fn wrong_password_and_unknown_email_get_the_same_answer(db: PgPool) {
    let app = TestApp::new(db).await;

    let wrong_password = app.login(SUPER_USER_EMAIL, "not-the-password").await;
    let unknown_email = app.login("nobody@test.local", SUPER_USER_PASSWORD).await;

    assert_eq!(wrong_password.status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown_email.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_password.body, unknown_email.body);
}

#[sqlx::test]
async fn me_requires_a_live_bearer_token(db: PgPool) {
    let app = TestApp::new(db).await;
    let token = app.super_user_token().await;

    let anonymous = app.send("GET", "/api/v1/auth/me", &[], None).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    let forged = app
        .send(
            "GET",
            "/api/v1/auth/me",
            &[("authorization", &bearer(&token::generate()))],
            None,
        )
        .await;
    assert_eq!(forged.status, StatusCode::UNAUTHORIZED);

    let me = app
        .send(
            "GET",
            "/api/v1/auth/me",
            &[("authorization", &bearer(&token))],
            None,
        )
        .await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["email"], SUPER_USER_EMAIL);
}

#[sqlx::test]
async fn each_request_refreshes_last_activity(db: PgPool) {
    let app = TestApp::new(db).await;
    let token = app.super_user_token().await;

    sqlx::query("UPDATE auth_sessions SET last_activity_at = now() - interval '5 minutes'")
        .execute(&app.db)
        .await
        .expect("age session");
    app.send(
        "GET",
        "/api/v1/auth/me",
        &[("authorization", &bearer(&token))],
        None,
    )
    .await;

    let recent: bool = sqlx::query_scalar(
        "SELECT last_activity_at > now() - interval '1 minute' FROM auth_sessions",
    )
    .fetch_one(&app.db)
    .await
    .expect("read session");
    assert!(recent);
}

#[sqlx::test]
async fn idle_and_expired_sessions_are_rejected(db: PgPool) {
    let app = TestApp::new(db).await;

    let idle = app.super_user_token().await;
    sqlx::query(
        "UPDATE auth_sessions SET last_activity_at = now() - make_interval(mins => $1 + 1)
         WHERE token_hash = $2",
    )
    .bind(IDLE_TIMEOUT_MINS)
    .bind(token::hash(&idle))
    .execute(&app.db)
    .await
    .expect("idle session");

    let expired = app.super_user_token().await;
    sqlx::query("UPDATE auth_sessions SET expires_at = now() WHERE token_hash = $1")
        .bind(token::hash(&expired))
        .execute(&app.db)
        .await
        .expect("expire session");

    for token in [idle, expired] {
        let response = app
            .send(
                "GET",
                "/api/v1/auth/me",
                &[("authorization", &bearer(&token))],
                None,
            )
            .await;
        assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    }
}

#[sqlx::test]
async fn deactivating_a_user_kills_their_sessions_immediately(db: PgPool) {
    let app = TestApp::new(db).await;
    let token = app.super_user_token().await;

    sqlx::query("UPDATE users SET is_active = false")
        .execute(&app.db)
        .await
        .expect("deactivate user");

    let response = app
        .send(
            "GET",
            "/api/v1/auth/me",
            &[("authorization", &bearer(&token))],
            None,
        )
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn logout_revokes_only_that_session(db: PgPool) {
    let app = TestApp::new(db).await;
    let laptop = app.super_user_token().await;
    let phone = app.super_user_token().await;

    let logout = app
        .send(
            "POST",
            "/api/v1/auth/logout",
            &[("authorization", &bearer(&laptop))],
            None,
        )
        .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);

    let laptop_me = app
        .send(
            "GET",
            "/api/v1/auth/me",
            &[("authorization", &bearer(&laptop))],
            None,
        )
        .await;
    let phone_me = app
        .send(
            "GET",
            "/api/v1/auth/me",
            &[("authorization", &bearer(&phone))],
            None,
        )
        .await;
    assert_eq!(laptop_me.status, StatusCode::UNAUTHORIZED);
    assert_eq!(phone_me.status, StatusCode::OK);
}
