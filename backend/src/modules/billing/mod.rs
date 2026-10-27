//! Plans, subscriptions and the allowances document tenants enforce.
//! Stripe will live in `infrastructure/stripe.rs` once it's approved.

pub mod application;
pub mod domain;
mod infrastructure;

pub use infrastructure::http::{internal_router, router};
