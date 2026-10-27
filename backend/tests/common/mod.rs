// Each test binary uses a different subset of these helpers.
#![allow(dead_code)]

use admin::{
    modules::{identity::application::create_super_user, tenants::TenantClient},
    platform::{
        mail::{Email, Mailer},
        rate_limit::RateLimiter,
        AppState,
    },
};
use axum::{
    body::{to_bytes, Body},
    http::{header, Request, StatusCode},
    Router,
};
use std::sync::{Arc, Mutex};

use ed25519_dalek::SigningKey;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

pub const SUPER_USER_EMAIL: &str = "super.user@test.local";
pub const SUPER_USER_PASSWORD: &str = "correct-horse-battery";
pub const PUBLIC_URL: &str = "https://admin.test";
/// Admin's private signing key in tests.
pub const SIGNING_SEED: [u8; 32] = [7; 32];

/// The real router on a fresh database from `#[sqlx::test]`, super user included.
pub struct TestApp {
    router: Router,
    pub db: PgPool,
    /// Every email the app "sent".
    pub outbox: Arc<Mutex<Vec<Email>>>,
}

pub struct TestResponse {
    pub status: StatusCode,
    pub body: Value,
}

impl TestApp {
    pub async fn new(db: PgPool) -> Self {
        Self::with_limiter(db, RateLimiter::disabled()).await
    }

    pub async fn with_limiter(db: PgPool, limiter: RateLimiter) -> Self {
        create_super_user(&db, SUPER_USER_EMAIL, SUPER_USER_PASSWORD.into())
            .await
            .expect("create super user");
        let (mailer, outbox) = Mailer::capture();
        Self {
            router: admin::app(AppState {
                db: db.clone(),
                tenant_client: TenantClient::new(Some(SigningKey::from_bytes(&SIGNING_SEED)))
                    .expect("tenant client"),
                limiter,
                mailer,
                public_url: PUBLIC_URL.into(),
            }),
            db,
            outbox,
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

impl TestApp {
    /// The token from the link in the last email sent to `to`.
    pub fn link_token(&self, to: &str) -> Option<String> {
        let outbox = self.outbox.lock().ok()?;
        let email = outbox.iter().rev().find(|e| e.to == to)?;
        let (_, rest) = email.body.split_once("/reset-password?token=")?;
        rest.split_whitespace().next().map(str::to_string)
    }

    pub fn emails_to(&self, to: &str) -> usize {
        self.outbox
            .lock()
            .map(|outbox| outbox.iter().filter(|e| e.to == to).count())
            .unwrap_or_default()
    }
}

pub fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// Creates a tenant through the API and returns `(id, api_key)`.
pub async fn new_tenant(app: &TestApp, auth: &str, slug: &str) -> (String, String) {
    let created = app
        .send(
            "POST",
            "/api/v1/tenants",
            &[("authorization", auth)],
            Some(serde_json::json!({ "slug": slug, "name": slug })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    (
        created.body["tenant"]["id"]
            .as_str()
            .expect("id")
            .to_string(),
        created.body["api_key"].as_str().expect("key").to_string(),
    )
}

/// A limiter on the local Redis (`REDIS_TEST_URL`, default
/// `redis://127.0.0.1:6379`), with its own key prefix so tests running side
/// by side don't count against each other.
pub async fn redis_limiter() -> RateLimiter {
    let url = std::env::var("REDIS_TEST_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let prefix = format!("test-{}", uuid::Uuid::new_v4());
    RateLimiter::connect(&url, &prefix)
        .await
        .expect("these tests need Redis")
}
