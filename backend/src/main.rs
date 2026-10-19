use std::time::Duration;

use admin::{
    modules::identity::application,
    platform::{db, error::AppError, AppState},
};
use tokio::signal::unix::{signal, SignalKind};

#[derive(Debug, thiserror::Error)]
enum StartupError {
    #[error("can't read .env: {0}")]
    Env(#[source] dotenvy::Error),
    #[error(transparent)]
    Db(#[from] db::DbError),
    #[error("failed to create super user: {0}")]
    SuperUser(#[source] AppError),
    #[error("failed to bind {addr}: {source}")]
    Bind {
        addr: String,
        #[source]
        source: std::io::Error,
    },
    #[error("server error: {0}")]
    Serve(#[source] std::io::Error),
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("fatal startup error: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), StartupError> {
    // No .env in production; config comes straight from the environment.
    // A .env that exists but doesn't parse is a mistake worth stopping for.
    if let Err(err) = dotenvy::dotenv() {
        if !err.not_found() {
            return Err(StartupError::Env(err));
        }
    }
    init_tracing();

    let pool = db::connect().await?;
    db::migrate(&pool).await?;
    application::ensure_super_user(&pool)
        .await
        .map_err(StartupError::SuperUser)?;

    let state = AppState { db: pool };
    tokio::spawn(purge_sessions(state.clone()));

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8081".to_string());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|source| StartupError::Bind {
            addr: bind_addr.clone(),
            source,
        })?;

    tracing::info!(%bind_addr, "admin service listening");

    axum::serve(listener, admin::app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(StartupError::Serve)
}

async fn purge_sessions(state: AppState) {
    let mut tick = tokio::time::interval(Duration::from_secs(15 * 60));
    loop {
        tick.tick().await;
        match application::purge_expired_sessions(&state.db).await {
            Ok(0) => {}
            Ok(removed) => tracing::info!(removed, "purged expired sessions"),
            Err(err) => tracing::warn!(%err, "failed to purge expired sessions"),
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let _ = tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .try_init();
}

async fn shutdown_signal() {
    let sigterm = async {
        match signal(SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(err) => {
                tracing::error!(%err, "can't listen for SIGTERM, only ctrl-c will stop the server");
                std::future::pending::<()>().await;
            }
        }
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = sigterm => {}
    }

    tracing::info!("shutting down, draining in-flight requests");
}
