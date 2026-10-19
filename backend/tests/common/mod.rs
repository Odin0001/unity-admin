// Each test binary uses a different subset of these helpers.
#![allow(dead_code)]

use admin::{modules::identity::application::create_super_user, platform::AppState};
use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    Router,
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

pub const SUPER_USER_EMAIL: &str = "super.user@test.local";
pub const SUPER_USER_PASSWORD: &str = "correct-horse-battery";

/// The real router on a fresh database from `#[sqlx::test]`, super user included.
pub struct TestApp {
    router: Router,
    pub db: PgPool,
}

pub struct TestResponse {
    pub status: StatusCode,
    pub body: Value,
}

impl TestApp {
    pub async fn new(db: PgPool) -> Self {
        create_super_user(&db, SUPER_USER_EMAIL, SUPER_USER_PASSWORD.into())
            .await
            .expect("create super user");
        Self {
            router: admin::app(AppState { db: db.clone() }),
            db,
        }
    }

    pub async fn send(
        &self,
        method: &str,
        uri: &str,
        headers: &[(&str, &str)],
        body: Option<Value>,
    ) -> TestResponse {
        let mut request = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let request = match body {
            Some(json) => request
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string())),
            None => request.body(Body::empty()),
        }
        .expect("valid request");

        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router is infallible");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("readable body");
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

        TestResponse { status, body }
    }

    pub async fn login(&self, email: &str, password: &str) -> TestResponse {
        let credentials = serde_json::json!({ "email": email, "password": password });
        self.send("POST", "/api/v1/auth/login", &[], Some(credentials))
            .await
    }

    pub async fn super_user_token(&self) -> String {
        let response = self.login(SUPER_USER_EMAIL, SUPER_USER_PASSWORD).await;
        assert_eq!(response.status, StatusCode::OK, "{}", response.body);
        response.body["token"]
            .as_str()
            .expect("token in login response")
            .to_string()
    }
}

pub fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}
