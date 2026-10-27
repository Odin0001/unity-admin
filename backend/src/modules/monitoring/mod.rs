//! Heartbeats from tenant deployments, and the overview of which tenants
//! are healthy and which have gone quiet.

pub mod application;
pub mod domain;
mod infrastructure;

pub use infrastructure::http::{internal_router, router};
