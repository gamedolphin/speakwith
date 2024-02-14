CREATE TABLE IF NOT EXISTS uploads (
       id TEXT NOT NULL PRIMARY KEY,
       uploaded_by TEXT NOT NULL,
       room_id TEXT,
       filename TEXT NOT NULL,
       url TEXT NOT NULL,
       created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS uploads_room_index ON uploads(room_id);
CREATE INDEX IF NOT EXISTS uploads_user_index ON uploads(uploaded_by);
