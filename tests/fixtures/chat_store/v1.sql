-- Frozen copy of a schema v1 chat.db, as a v1-only build left it. Do not edit:
-- the migration tests rely on this exact shape and these exact rows.
CREATE TABLE conversations (
    id         INTEGER PRIMARY KEY,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    profile    TEXT NOT NULL CHECK (profile IN ('analyst', 'programmer')),
    title      TEXT NOT NULL
);
CREATE TABLE messages (
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    seq             INTEGER NOT NULL,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content         TEXT NOT NULL,
    engine          TEXT NOT NULL,
    model           TEXT,
    status          TEXT NOT NULL
        CHECK (status IN ('complete', 'cancelled', 'interrupted', 'truncated', 'error')),
    created_at      INTEGER NOT NULL,
    UNIQUE (conversation_id, seq)
);
CREATE TABLE tags (
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    tag             TEXT NOT NULL,
    is_project      INTEGER NOT NULL CHECK (is_project IN (0, 1)),
    PRIMARY KEY (conversation_id, tag)
) WITHOUT ROWID;
CREATE INDEX tags_by_tag ON tags(tag);
CREATE INDEX conversations_by_activity ON conversations(updated_at);

INSERT INTO conversations(id, created_at, updated_at, profile, title) VALUES
  (1, 1000, 1500, 'programmer', 'Fix the parser'),
  (2, 2000, 2600, 'analyst', 'Quarterly numbers'),
  (7, 3000, 3000, 'programmer', 'Empty chat');
INSERT INTO messages(conversation_id, seq, role, content, engine, model, status, created_at) VALUES
  (1, 1, 'user', 'why does it panic?', 'ollama', NULL, 'complete', 1001),
  (1, 2, 'assistant', 'an unwrap on line 4', 'ollama', 'qwen3', 'complete', 1002),
  (1, 3, 'user', 'and now?', 'ollama', NULL, 'complete', 1500),
  (2, 1, 'user', 'sum Q3', 'claude', NULL, 'complete', 2001),
  (2, 2, 'assistant', 'partial answ', 'claude', 'sonnet', 'truncated', 2002),
  (2, 3, 'assistant', 'oops', 'claude', 'sonnet', 'error', 2600);
INSERT INTO tags(conversation_id, tag, is_project) VALUES
  (1, 'rust', 0), (1, 'lazarobox', 1), (2, 'finance', 0);

PRAGMA user_version = 1;
