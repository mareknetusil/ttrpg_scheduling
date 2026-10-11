use argon2::{
    Argon2,
    password_hash::{Error as ArgError, PasswordHasher, PasswordVerifier, phc::PasswordHash},
};

pub async fn hash(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        let argon2 = Argon2::default();
        Ok(argon2.hash_password(password.as_bytes())?.to_string())
    })
    .await?
}

pub async fn verify(password: String, hash: String) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || {
        let parsed_hash = PasswordHash::new(&hash)?;
        let argon2 = Argon2::default();
        match argon2.verify_password(password.as_bytes(), &parsed_hash) {
            Ok(()) => Ok(true),
            Err(ArgError::PasswordInvalid) => Ok(false),
            Err(e) => Err(e.into()),
        }
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_then_verify() {
        let h = hash("hunter2".to_string()).await.unwrap();
        assert!(verify("hunter2".to_string(), h.clone()).await.unwrap());
        assert!(!verify("wrong".to_string(), h).await.unwrap());
    }
}
