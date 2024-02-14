use anyhow::Result;
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2, PasswordHash, PasswordVerifier,
};
use rand_core::OsRng;
use sqlx::{Sqlite, Transaction};
use thiserror::Error;
use time::OffsetDateTime;

use crate::Database;

#[derive(sqlx::FromRow, serde::Serialize, Debug)]
pub struct UserCombined {
    pub id: String,
    pub email: String,
    pub is_admin: bool,
    pub is_enabled: bool,
    pub created_at: OffsetDateTime,
    pub username: String,
    pub bio: Option<String>,
    pub image: Option<String>,
}

pub struct User {
    pub id: String,
    pub email: String,
    pub password: String,
    pub hash: String,
    pub is_admin: bool,
    pub is_enabled: bool,
    pub created_at: OffsetDateTime,
}

impl Default for User {
    fn default() -> Self {
        Self {
            id: Default::default(),
            email: Default::default(),
            password: Default::default(),
            hash: Default::default(),
            is_admin: false,
            is_enabled: false,
            created_at: OffsetDateTime::now_utc(),
        }
    }
}

#[derive(Default)]
pub struct UserProfile {
    pub id: String,
    pub user_id: String,
    pub username: String,
    pub bio: Option<String>,
    pub image: Option<String>,
}

pub async fn has_admin(db: &Database) -> Result<bool> {
    let res = sqlx::query!("SELECT COUNT(*) user_count FROM users")
        .fetch_one(&db.pool)
        .await?;

    let count = res.user_count;

    Ok(count > 0)
}

pub async fn search_users(db: &Database, search: &str, user_id: &str) -> Result<Vec<UserCombined>> {
    let search = format!("%{}%", search);
    let output = sqlx::query_as!(
        UserCombined,
        r#"
SELECT u.id, u.email, u.is_admin as "is_admin!", u.is_enabled as "is_enabled!", u.created_at as "created_at!", p.username as username, p.bio, p.image
FROM users AS u 
INNER JOIN user_profiles AS p ON u.id = p.user_id
WHERE (p.username LIKE $1) AND u.id != $2
LIMIT 5
"#,
        search,
        user_id
    ).fetch_all(&db.pool).await?;

    Ok(output)
}

pub async fn update_user_password(
    db: &Database,
    email: &str,
    current: &str,
    update: &str,
) -> Result<()> {
    let user_id = verify_userpassword(db, email, current).await?;

    let (password, salt) = hash_password(update)?;

    sqlx::query!(
        r#"
UPDATE users
SET password = $1, hash = $2
WHERE id = $3
"#,
        password,
        salt,
        user_id
    )
    .execute(&db.pool)
    .await?;

    Ok(())
}

pub async fn update_user_profile(
    db: &Database,
    user_id: &str,
    name: &str,
    bio: &str,
) -> Result<UserCombined> {
    sqlx::query!(
        r#"
UPDATE user_profiles
SET username = $1, bio = $2
WHERE user_id = $3
"#,
        name,
        bio,
        user_id
    )
    .execute(&db.pool)
    .await?;

    get_user_with_profile(db, user_id).await
}

pub async fn set_user_image<'a>(
    trx: &mut Transaction<'a, Sqlite>,
    user_id: &str,
    url: Option<&str>,
) -> Result<()> {
    sqlx::query!(
        r#"
UPDATE user_profiles
SET image = $1
WHERE user_id = $2
"#,
        url,
        user_id
    )
    .execute(&mut **trx)
    .await?;

    Ok(())
}

pub async fn unset_user_image<'a>(db: &Database, user_id: &str) -> Result<()> {
    sqlx::query!(
        r#"
UPDATE user_profiles
SET image = $1
WHERE user_id = $2
"#,
        None::<String>,
        user_id
    )
    .execute(&db.pool)
    .await?;

    Ok(())
}

pub async fn enable_user(db: &Database, user_id: &str, enable: bool) -> Result<()> {
    sqlx::query!(
        r#"
UPDATE users
SET is_enabled = $1
WHERE id = $2;
"#,
        enable,
        user_id
    )
    .execute(&db.pool)
    .await?;

    Ok(())
}

#[derive(sqlx::FromRow)]
pub struct NameQuery {
    username: String,
}

pub async fn get_usernames_by_id(db: &Database, users: &str) -> Result<Vec<String>> {
    println!("users:{}", users);
    let query = format!(
        "SELECT username FROM user_profiles WHERE user_id IN ({})",
        users
    );
    let res: Vec<NameQuery> = sqlx::query_as(&query).fetch_all(&db.pool).await?;

    let output = res.into_iter().map(|v| v.username).collect::<Vec<String>>();

    Ok(output)
}

pub async fn make_admin_user(db: &Database, user_id: &str, enable: bool) -> Result<()> {
    sqlx::query!(
        r#"
UPDATE users
SET is_admin = $1
WHERE id = $2;
"#,
        enable,
        user_id
    )
    .execute(&db.pool)
    .await?;

    Ok(())
}

pub async fn create_user<'a>(
    user_id: &str,
    mut trx: Transaction<'a, Sqlite>,
    user: User,
    profile: UserProfile,
) -> Result<(String, Transaction<'a, Sqlite>)> {
    let (password, salt) = hash_password(&user.password)?;

    sqlx::query_as!(
        User,
        r#"
INSERT INTO users (id, email, password, hash, is_admin, is_enabled)
VALUES ($1, $2, $3, $4, $5, $6)
"#,
        user_id,
        user.email,
        password,
        salt,
        user.is_admin,
        user.is_enabled,
    )
    .execute(&mut *trx)
    .await?;

    let profile_id = xid::new().to_string();
    sqlx::query_as!(
        UserProfile,
        r#"
INSERT INTO user_profiles (id, user_id, username, bio, image)
VALUES ($1 ,$2, $3, $4, $5)
"#,
        profile_id,
        user_id,
        profile.username,
        profile.bio,
        profile.image,
    )
    .execute(&mut *trx)
    .await?;

    Ok((user_id.to_string(), trx))
}

pub async fn get_user_list(db: &Database) -> Result<Vec<UserCombined>> {
    sqlx::query_as!(
        UserCombined,
        r#"
SELECT u.id, u.email, u.is_admin as "is_admin!", u.is_enabled as "is_enabled!", u.created_at as "created_at!", p.username as username, p.bio, p.image
FROM users AS u 
INNER JOIN user_profiles AS p ON u.id = p.user_id
"#,        
    )
    .fetch_all(&db.pool)
    .await.map_err(|e| e.into())
}

pub async fn get_user_with_profile(db: &Database, userid: &str) -> Result<UserCombined> {
    sqlx::query_as!(
        UserCombined,
        r#"
SELECT u.id, u.email, u.is_admin as "is_admin!", u.is_enabled as "is_enabled!", u.created_at as "created_at!", p.username as username, p.bio, p.image
FROM users AS u 
INNER JOIN user_profiles AS p ON u.id = p.user_id
WHERE u.id = $1
"#,
        userid
    )
    .fetch_one(&db.pool)
    .await.map_err(|e| e.into())
}

pub async fn verify_userpassword(
    db: &Database,
    email: &str,
    password: &str,
) -> Result<String, DBUserErrors> {
    let user = sqlx::query!(
        r#"SELECT id, is_enabled as "is_enabled!", password FROM users WHERE email = $1"#,
        email
    )
    .fetch_one(&db.pool)
    .await
    .map_err(|e| DBUserErrors::InternalError(e.into()))?;

    if !user.is_enabled {
        return Err(DBUserErrors::UserNotEnabled);
    }

    let existing =
        String::from_utf8(user.password).map_err(|e| DBUserErrors::InternalError(e.into()))?;
    if compare_password(&existing, password) {
        return Ok(user.id);
    }

    Err(DBUserErrors::PasswordMismatch(password.to_string()))
}

fn hash_password(password: &str) -> Result<(String, String), DBUserErrors> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(DBUserErrors::PasswordHashFailed)?
        .to_string();

    Ok((password_hash, salt.to_string()))
}

fn compare_password(hashed: &str, password: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(hashed) else {
        return false;
    };

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

#[derive(Debug, Error)]
pub enum DBUserErrors {
    #[error("failed to hash password: {0}")]
    PasswordHashFailed(argon2::password_hash::Error),
    #[error("password mismatch: {0}")]
    PasswordMismatch(String),
    #[error("user not enabled")]
    UserNotEnabled,
    #[error("internal error: {0}")]
    InternalError(anyhow::Error),
}
