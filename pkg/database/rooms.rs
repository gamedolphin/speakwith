use anyhow::Result;
use sqlx::{Pool, Sqlite};
use thiserror::Error;
use time::OffsetDateTime;

use crate::Database;

#[derive(serde::Serialize)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub is_private: bool,
    pub is_user: bool,
    pub description: String,
    pub created_at: OffsetDateTime,
}

#[derive(Error, Debug)]
pub enum RoomError {
    #[error("room cannot be empty")]
    CannotBeEmpty,
}

pub async fn init_rooms(pool: &Pool<Sqlite>) -> Result<()> {
    let mut tx = pool.begin().await?;

    let None = sqlx::query_as!(
        Room,
        r#"
SELECT id, name, description, is_user as "is_user!", is_private as "is_private!", created_at as "created_at!" FROM rooms
"#
    )
    .fetch_one(&mut *tx)
    .await
    .ok() else {
        // some rooms exists
        return Ok(());
    };

    let general = Room {
        id: "general".into(),
        name: "general".into(),
        is_private: false,
        is_user: false,
        description: "The general channel".to_string(),
        created_at: OffsetDateTime::now_utc(),
    };

    sqlx::query!(
        r#"
INSERT INTO rooms (id, name, description)
VALUES ($1, $2, $3)"#,
        general.id,
        general.name,
        general.description,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(())
}

#[derive(sqlx::FromRow, serde::Serialize, Debug)]
pub struct RoomUser {
    pub id: String,
    pub name: String,
}

pub async fn add_user_to_room(db: &Database, roomid: &str, userid: &str) -> Result<()> {
    sqlx::query!(
        r#"INSERT INTO user_rooms (user_id, room_id) VALUES ($1, $2)"#,
        userid,
        roomid
    )
    .execute(&db.pool)
    .await?;

    Ok(())
}

pub async fn remove_user_from_room(db: &Database, roomid: &str, userid: &str) -> Result<()> {
    let total = sqlx::query!(
        "SELECT COUNT(*) as count FROM user_rooms WHERE room_id = $1",
        roomid
    )
    .fetch_one(&db.pool)
    .await?;

    if total.count == 1 {
        return Err(RoomError::CannotBeEmpty.into());
    }

    sqlx::query!(
        r#"DELETE FROM user_rooms WHERE user_id = $1 AND room_id = $2"#,
        userid,
        roomid
    )
    .execute(&db.pool)
    .await?;

    Ok(())
}

pub async fn is_member_of_room(db: &Database, roomid: &str, userid: &str) -> bool {
    let output = sqlx::query!(
        "SELECT user_id FROM user_rooms WHERE room_id = $1 AND user_id = $2",
        roomid,
        userid
    )
    .fetch_one(&db.pool)
    .await;

    output.is_ok()
}

pub async fn get_room_users(db: &Database, roomid: &str) -> Result<Vec<RoomUser>> {
    let users = sqlx::query_as!(
        RoomUser,
        r#"
SELECT ur.user_id as "id!", p.username as "name!" 
FROM user_rooms ur
LEFT JOIN user_profiles AS p ON ur.user_id = p.user_id
WHERE ur.room_id = $1
"#,
        roomid
    )
    .fetch_all(&db.pool)
    .await?;

    Ok(users)
}

pub async fn get_room(db: &Database, roomid: &str, user_id: &str) -> Result<Room> {
    let room = sqlx::query_as!(
        Room,
        r#"
SELECT id, description, name, is_user as "is_user!", is_private as "is_private!", created_at as "created_at!"
FROM rooms r
LEFT JOIN user_rooms ur ON r.id = ur.room_id AND ur.user_id = $1
WHERE r.id = $2 AND (r.is_private = FALSE OR ur.user_id IS NOT NULL);
"#,
        user_id,
        roomid
    )
    .fetch_one(&db.pool)
    .await?;

    Ok(room)
}

pub async fn create_room(
    db: &Database,
    id: &str,
    name: &str,
    description: &str,
    is_private: bool,
    is_user: bool,
    users: &[String],
) -> Result<String> {
    let mut tx = db.pool.begin().await?;

    if sqlx::query!("SELECT id FROM rooms WHERE id = $1", id)
        .fetch_one(&mut *tx)
        .await
        .is_ok()
    {
        // room with this id already exists, return early
        return Ok(id.to_string());
    };

    let room = Room {
        id: id.into(),
        name: name.into(),
        is_user,
        is_private,
        description: description.to_string(),
        created_at: OffsetDateTime::now_utc(),
    };

    sqlx::query!(
        r#"
INSERT INTO rooms (id, name, description, is_private, is_user)
VALUES ($1, $2, $3, $4, $5)"#,
        room.id,
        room.name,
        room.description,
        room.is_private,
        room.is_user,
    )
    .execute(&mut *tx)
    .await?;

    if room.is_private {
        for user_id in users {
            sqlx::query!(
                r#"
INSERT INTO user_rooms (room_id, user_id)
VALUES ($1, $2)
"#,
                id,
                user_id
            )
            .execute(&mut *tx)
            .await?;
        }
    }

    tx.commit().await?;

    Ok(id.to_string())
}

pub async fn get_rooms(db: &Database, user_id: &str) -> Result<(Vec<Room>, Vec<Room>)> {
    let rooms = sqlx::query_as!(
        Room,
        r#"
SELECT id, description, name, is_user as "is_user!", is_private as "is_private!", created_at as "created_at!"
FROM rooms r
LEFT JOIN user_rooms ur ON r.id = ur.room_id AND ur.user_id = $1
WHERE r.is_private = FALSE OR ur.user_id IS NOT NULL
"#,
        user_id
    )
    .fetch_all(&db.pool)
        .await?;

    let rooms = rooms.into_iter().partition(|v| v.is_user);

    Ok(rooms)
}
