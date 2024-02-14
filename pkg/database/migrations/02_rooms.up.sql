CREATE TABLE IF NOT EXISTS rooms (
       id TEXT NOT NULL PRIMARY KEY,
       name TEXT NOT NULL,
       description TEXT NOT NULL,
       is_private BOOLEAN DEFAULT FALSE,
       is_user BOOLEAN DEFAULT FALSE,
       created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS user_rooms (
       user_id TEXT NOT NULL,
       room_id TEXT NOT NULL,
       PRIMARY KEY (user_id, room_id),
       FOREIGN KEY (user_id) REFERENCES users(id),
       FOREIGN KEY (room_id) REFERENCES rooms(id)
);

CREATE TABLE IF NOT EXISTS messages (
       id TEXT NOT NULL PRIMARY KEY,
       room_id TEXT NOT NULL REFERENCES rooms(id),
       user_id TEXT NOT NULL REFERENCES users(id),
       created_at DATETIME DEFAULT CURRENT_TIMESTAMP,

       message TEXT
);

CREATE INDEX IF NOT EXISTS message_room_index ON messages(room_id);
CREATE INDEX IF NOT EXISTS message_user_index ON messages(user_id);
