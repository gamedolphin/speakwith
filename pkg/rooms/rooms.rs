use std::collections::HashMap;

use anyhow::Result;
use database::{messages::ChatMessage, Database};
use parking_lot::RwLock;
use thiserror::Error;
use time::OffsetDateTime;
use tokio::sync::broadcast::{Receiver, Sender};

pub struct Room {
    pub room_id: String,
    pub sender: Sender<ChatMessage>,
    pub db: Database,
}

pub struct Manager {
    db: Database,
    rooms: RwLock<HashMap<String, Room>>,
}

impl Manager {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            rooms: Default::default(),
        }
    }

    pub async fn join_room(&self, room_id: String, user_id: &str) -> Result<Receiver<ChatMessage>> {
        let _ = database::rooms::get_room(&self.db, &room_id, user_id).await?;

        let mut rooms = self.rooms.write();
        let room = rooms.entry(room_id.clone()).or_insert_with(move || {
            let (sender, _) = tokio::sync::broadcast::channel::<ChatMessage>(1000);
            Room {
                room_id,
                sender,
                db: self.db.clone(),
            }
        });

        let recv = room.sender.subscribe();
        Ok(recv)
    }

    pub async fn get_room_messages(
        &self,
        room_id: &str,
        page: i32,
        user_id: &str,
    ) -> Result<Vec<ChatMessage>> {
        let msgs =
            database::messages::get_messages_for_room(&self.db, room_id, user_id, page).await?;

        Ok(msgs)
    }

    pub async fn send_message(
        &self,
        room_id: &str,
        user_id: &str,
        user_name: &str,
        user_image: Option<String>,
        message: &str,
    ) -> Result<()> {
        let id = database::messages::send_message(&self.db, room_id, user_id, message).await?;

        let rooms = self.rooms.read();
        let room = rooms
            .get(room_id)
            .ok_or_else(|| ChatRoomErrors::RoomEmpty(room_id.to_string()))?;

        let obj = ChatMessage {
            id,
            room_id: room_id.to_string(),
            user_id: user_id.to_string(),
            user_name: user_name.to_string(),
            user_image,
            created_at: OffsetDateTime::now_utc(),
            message: message.to_string(),
        };

        room.sender.send(obj)?;

        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum ChatRoomErrors {
    #[error("room not joined : {0}")]
    RoomEmpty(String),
}
