//! Tenant registry. Master-tenant users manage it; tenant deployments call
//! back in with the API key they got at creation, and admin calls them with
//! signed requests.

pub mod application;
pub mod domain;
mod infrastructure;

pub use infrastructure::{
    http::{directory_router, internal_router, router, CallingTenant},
    tenant_client::{signed_message, TenantClient, TenantClientError},
};
