//! Argon2id hashing. It's slow on purpose, so it runs on the blocking pool;
//! that's also why the functions take owned strings.

use argon2::{
    password_hash::{
        self, rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
    },
    Argon2,
};

#[derive(Debug, thiserror::Error)]
pub enum PasswordError {
    #[error("password hash error: {0}")]
    Hash(password_hash::Error),
    #[error("password hashing task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

/// ```
/// use admin::platform::password;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), password::PasswordError> {
/// let stored = password::hash("correct horse".into()).await?;
/// assert!(password::verify("correct horse".into(), stored.clone()).await?);
/// assert!(!password::verify("wrong horse".into(), stored).await?);
/// # Ok(())
/// # }
/// ```
pub async fn hash(password: String) -> Result<String, PasswordError> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(PasswordError::Hash)
    })
    .await?
}

/// A wrong password is `Ok(false)`. `Err` means the stored hash is broken.
pub async fn verify(password: String, stored_hash: String) -> Result<bool, PasswordError> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&stored_hash).map_err(PasswordError::Hash)?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn salts_every_hash() -> Result<(), PasswordError> {
        let a = hash("same password".into()).await?;
        let b = hash("same password".into()).await?;
        assert_ne!(a, b);
        Ok(())
    }

    #[tokio::test]
    async fn broken_stored_hash_is_an_error() {
        let result = verify("anything".into(), "not-a-phc-string".into()).await;
        assert!(matches!(result, Err(PasswordError::Hash(_))));
    }
}
