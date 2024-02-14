use anyhow::Result;
use time::OffsetDateTime;

use crate::Database;

pub const MAX_FETCH: i32 = 5;

#[derive(Clone, Debug, serde::Serialize)]
pub struct ChatMessage {
    pub id: String,
    pub room_id: String,
    pub user_id: String,
    pub user_name: String,
    pub user_image: Option<String>,
    pub created_at: OffsetDateTime,

    pub message: String,
}

pub async fn get_messages_for_room(
    db: &Database,
    room_id: &str,
    user_id: &str,
    page: i32,
) -> Result<Vec<ChatMessage>> {
    let offset = page * MAX_FETCH;
    let messages = sqlx::query_as!(
        ChatMessage,
        r#"
SELECT m.id, m.room_id, m.user_id, m.created_at as "created_at!", m.message as "message!", user_profiles.username as "user_name", user_profiles.image as "user_image"
FROM messages m
INNER JOIN user_profiles ON user_profiles.user_id = m.user_id
JOIN (
    SELECT r.id
    FROM rooms r
    LEFT JOIN user_rooms ur ON r.id = ur.room_id AND ur.user_id = $4
    WHERE r.id = $1 AND (r.is_private = FALSE OR ur.user_id IS NOT NULL)
) AS accessible_rooms ON m.room_id = accessible_rooms.id
WHERE room_id = $1
ORDER BY created_at DESC
LIMIT $2
OFFSET $3;
"#,
        room_id,
        MAX_FETCH,
        offset,
        user_id,
    )
    .fetch_all(&db.pool)
    .await?;

    Ok(messages)
}

pub async fn send_message(
    db: &Database,
    room_id: &str,
    user_id: &str,
    message: &str,
) -> Result<String> {
    sqlx::query!(
        r#"
SELECT r.id
FROM rooms r
LEFT JOIN user_rooms ur ON r.id = ur.room_id AND ur.user_id = $2
WHERE r.id = $1 AND (r.is_private = FALSE OR ur.user_id IS NOT NULL)
"#,
        room_id,
        user_id
    )
    .fetch_one(&db.pool)
    .await?;

    let id = xid::new().to_string();

    sqlx::query!(
        r#"
INSERT INTO messages (id, room_id, user_id, message)
VALUES ($1, $2, $3, $4)
"#,
        id,
        room_id,
        user_id,
        message
    )
    .execute(&db.pool)
    .await?;

    Ok(id)
}
