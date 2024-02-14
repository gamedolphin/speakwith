CREATE TABLE IF NOT EXISTS users (
  id TEXT NOT NULL PRIMARY KEY,
  email TEXT NOT NULL UNIQUE,
  password BLOB NOT NULL,
  hash BLOB NOT NULL,
  is_admin BOOLEAN DEFAULT FALSE,
  is_enabled BOOLEAN DEFAULT FALSE,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS user_email_index ON users(email);

CREATE TABLE IF NOT EXISTS user_profiles (
  id TEXT NOT NULL PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  username TEXT NOT NULL,
  bio TEXT,
  image TEXT
);

CREATE INDEX IF NOT EXISTS user_profile_index ON user_profiles(user_id);
