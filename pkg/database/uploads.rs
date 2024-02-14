use anyhow::Result;
use sqlx::{Sqlite, Transaction};
use time::OffsetDateTime;

use crate::Database;

pub struct Upload {
    pub id: String,
    pub uploaded_by: String,
    pub room_id: Option<String>,
    pub url: String,
    pub created_at: OffsetDateTime,
}

pub async fn add_upload_and_continue<'a>(
    db: &'a Database,
    user_id: &str,
    room_id: Option<String>,
    filename: Option<String>,
    url: Option<String>,
) -> Result<(String, Transaction<'a, Sqlite>)> {
    let mut trx = db.pool.begin().await?;

    if url.is_none() {
        // just return, nothing to upload really
        return Ok(("".to_string(), trx));
    }

    let id = xid::new().to_string();
    sqlx::query!(
        r#"
INSERT INTO uploads (id, uploaded_by, room_id, filename, url)
VALUES ($1, $2, $3, $4, $5)
"#,
        id,
        user_id,
        room_id,
        filename,
        url,
    )
    .execute(&mut *trx)
    .await?;

    Ok((id, trx))
}
