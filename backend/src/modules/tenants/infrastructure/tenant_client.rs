//! Calls from admin to tenant deployments. Each one is signed with admin's
//! Ed25519 key, so a tenant can tell it really came from here and a leaked
//! tenant can't impersonate admin (`references/architecture.md`, section 2).

use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum TenantClientError {
    #[error("ADMIN_SIGNING_KEY must be 32 bytes, base64url without padding")]
    BadKey,
    #[error("ADMIN_SIGNING_KEY is not set, so tenants can't be called")]
    NoKey,
    #[error("tenant request failed: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Clone)]
pub struct TenantClient {
    http: reqwest::Client,
    key: Option<SigningKey>,
}

impl TenantClient {
    pub fn new(key: Option<SigningKey>) -> Result<Self, TenantClientError> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
            key,
        })
    }

    /// Reads the private key from `ADMIN_SIGNING_KEY`. Without one, admin
    /// still runs; it just can't call tenants.
    pub fn from_env() -> Result<Self, TenantClientError> {
        let Some(encoded) = std::env::var("ADMIN_SIGNING_KEY")
            .ok()
            .filter(|v| !v.is_empty())
        else {
            return Self::new(None);
        };
        let seed: [u8; 32] = URL_SAFE_NO_PAD
            .decode(encoded.trim())
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(TenantClientError::BadKey)?;
        Self::new(Some(SigningKey::from_bytes(&seed)))
    }

    /// What every tenant is configured with as `ADMIN_PUBLIC_KEY`.
    pub fn public_key(&self) -> Option<String> {
        let key = self.key.as_ref()?;
        Some(URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes()))
    }

    pub async fn refresh_allowances(&self, api_url: &str) -> Result<(), TenantClientError> {
        self.post(api_url, "/api/v1/internal/admin/refresh-allowances")
            .await
    }

    async fn post(&self, api_url: &str, path: &str) -> Result<(), TenantClientError> {
        let key = self.key.as_ref().ok_or(TenantClientError::NoKey)?;
        let timestamp = Utc::now().timestamp();
        let signature = key.sign(&signed_message("POST", path, timestamp, &[]));

        self.http
            .post(format!("{api_url}{path}"))
            .header("x-admin-timestamp", timestamp)
            .header(
                "x-admin-signature",
                URL_SAFE_NO_PAD.encode(signature.to_bytes()),
            )
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

/// The bytes admin signs and the tenant verifies:
/// `METHOD \n path \n unix-seconds \n hex(sha256(body))`.
pub fn signed_message(method: &str, path: &str, timestamp: i64, body: &[u8]) -> Vec<u8> {
    let body_hash = hex::encode(Sha256::digest(body));
    format!("{method}\n{path}\n{timestamp}\n{body_hash}").into_bytes()
}
