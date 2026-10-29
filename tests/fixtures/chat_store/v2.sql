-- Frozen copy of a schema v2 chat.db with the nullable `folder` column added
-- without a version bump (chat-folder). Do not edit: the migration tests rely
-- on this exact shape and these exact rows.
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
CREATE TABLE memory_sessions (
    conversation_id INTEGER PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    session_id      TEXT NOT NULL UNIQUE,
    project         TEXT NOT NULL
);
CREATE TABLE memory_notes (
    project         TEXT NOT NULL,
    topic_key       TEXT NOT NULL,
    observation_id  INTEGER NOT NULL,
    conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    PRIMARY KEY (project, topic_key)
) WITHOUT ROWID;
CREATE INDEX memory_notes_by_conversation ON memory_notes(conversation_id);
ALTER TABLE conversations ADD COLUMN folder TEXT;

INSERT INTO conversations(id, created_at, updated_at, profile, title, folder) VALUES
  (1, 1000, 1500, 'programmer', 'Fix the parser', '/home/u/work/parser'),
  (2, 2000, 2600, 'analyst', 'Quarterly numbers', '/home/u/books'),
  (7, 3000, 3000, 'programmer', 'Empty chat', NULL);
INSERT INTO messages(conversation_id, seq, role, content, engine, model, status, created_at) VALUES
  (1, 1, 'user', 'why does it panic?', 'ollama', NULL, 'complete', 1001),
  (1, 2, 'assistant', 'an unwrap on line 4', 'ollama', 'qwen3', 'complete', 1002),
  (1, 3, 'user', 'and now?', 'ollama', NULL, 'complete', 1500),
  (2, 1, 'user', 'sum Q3', 'claude', NULL, 'complete', 2001),
  (2, 2, 'assistant', 'partial answ', 'claude', 'sonnet', 'truncated', 2002),
  (2, 3, 'assistant', 'oops', 'claude', 'sonnet', 'error', 2600);
INSERT INTO tags(conversation_id, tag, is_project) VALUES
  (1, 'rust', 0), (1, 'lazarobox', 1), (2, 'finance', 0);
INSERT INTO memory_sessions(conversation_id, session_id, project) VALUES
  (1, 'sess-1', 'lazarobox-shell'), (2, 'sess-2', 'books');
INSERT INTO memory_notes(project, topic_key, observation_id, conversation_id, title) VALUES
  ('lazarobox-shell', 'bug/parser', 41, 1, 'Parser unwrap'),
  ('books', 'q3/summary', 42, 2, 'Q3 summary');

PRAGMA user_version = 2;
