pub mod db;
pub mod error;
pub mod password;
pub mod token;

use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}
