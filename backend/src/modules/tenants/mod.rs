//! Tenant registry. Master-tenant users manage it; tenant deployments call
//! back in with the API key they got at creation.

pub mod application;
pub mod domain;
mod infrastructure;

pub use infrastructure::http::{internal_router, router};
