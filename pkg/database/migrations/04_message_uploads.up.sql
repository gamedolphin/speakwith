CREATE TABLE IF NOT EXISTS message_uploads (
       id TEXT NOT NULL PRIMARY KEY,
       message_id TEXT NOT NULL,
       upload_id TEXT NOT NULL,
       upload_path TEXT NOT NULL,
       created_at DATETIME DEFAULT CURRENT_TIMESTAMP,       
       FOREIGN KEY (message_id) REFERENCES messages(id),
       FOREIGN KEY (upload_id) REFERENCES uploads(id)                     
);

CREATE INDEX IF NOT EXISTS message_id_upload ON message_uploads(message_id);
CREATE INDEX IF NOT EXISTS upload_id_message ON message_uploads(upload_id);
