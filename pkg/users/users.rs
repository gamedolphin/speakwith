use std::collections::BTreeMap;

use anyhow::Result;
use database::Database;
use hmac::{Hmac, Mac};
use jwt::{SignWithKey, VerifyWithKey};
use sha2::Sha256;
use thiserror::Error;

#[derive(serde::Deserialize)]
pub struct RegisterForm {
    pub email: String,
    pub name: String,
    pub password: String,
}

#[derive(serde::Deserialize)]
pub struct LoginForm {
    pub email: String,
    pub password: String,
}

pub async fn login_user(db: &Database, secret: &str, form: LoginForm) -> Result<String> {
    let user_id = database::users::verify_userpassword(db, &form.email, &form.password).await?;

    generate_token(secret, &user_id)
}

pub fn generate_token(secret: &str, user_id: &str) -> Result<String> {
    let key: Hmac<Sha256> = Hmac::new_from_slice(secret.as_bytes())?;
    let mut claims = BTreeMap::new();
    claims.insert("sub", user_id);
    let token = claims.sign_with_key(&key)?;

    Ok(token)
}

pub fn extract_sub(secret: &str, token: &str) -> Result<Option<String>> {
    let key: Hmac<Sha256> = Hmac::new_from_slice(secret.as_bytes())?;
    let claims: BTreeMap<String, String> = token.verify_with_key(&key)?;
    let sub = claims.get("sub").cloned();
    Ok(sub)
}

#[derive(Error, Debug)]
pub enum UserErrors {
    #[error("internal error: {0}")]
    InternalError(anyhow::Error),
}
