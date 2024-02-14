use std::time::Duration;

use anyhow::Result;
use rooms::init_rooms;
use sqlx::{migrate::MigrateDatabase, sqlite::SqlitePoolOptions, Pool, Sqlite};

pub mod messages;
pub mod rooms;
pub mod uploads;
pub mod users;

#[derive(Clone)]
pub struct Database {
    pool: Pool<Sqlite>,
}

impl Database {
    pub async fn new(path: &str) -> Result<Database> {
        if !Sqlite::database_exists(path).await.unwrap_or(false) {
            Sqlite::create_database(path).await?;
        }

        let pool = SqlitePoolOptions::new()
            .acquire_timeout(Duration::from_secs(5))
            .connect(path)
            .await?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        init_rooms(&pool).await?;

        Ok(Database { pool })
    }
}
