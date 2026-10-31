CREATE TABLE user_pending_battleless (
    user_id INTEGER PRIMARY KEY,
    chapter_id INTEGER NOT NULL,
    episode_id INTEGER NOT NULL,
    multiplier INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
