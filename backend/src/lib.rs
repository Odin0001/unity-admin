//! Platform admin: the master tenant's console for the debt collection
//! platform.

pub mod modules;
pub mod platform;
pub mod shared_kernel;

use axum::{extract::State, http::StatusCode, routing::get, Router};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use modules::{identity, tenants};
use platform::{error::AppError, AppState};

pub fn app(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .nest("/auth", identity::router())
        .nest("/tenants", tenants::router())
        .nest("/internal", tenants::internal_router())
        .fallback(|| async { AppError::NotFound });

    // Paths outside the API that aren't real files get index.html, so
    // client-side routes survive a reload.
    let spa = ServeDir::new("web/dist").fallback(ServeFile::new("web/dist/index.html"));

    Router::new()
        .nest("/api/v1", api)
        .fallback_service(spa)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> StatusCode {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => StatusCode::OK,
        Err(err) => {
            tracing::error!(%err, "health check failed");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}
