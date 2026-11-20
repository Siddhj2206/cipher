//! The schema and its migrations. `docs/design/storage.md` explains every table.

use rusqlite::Connection;

use crate::{Error, Result};

/// One entry per schema version; `PRAGMA user_version` counts how many have run.
/// Never edit a migration once a real Book has been written with it: append a new one.
const MIGRATIONS: &[&str] = &[V1, V2, V3, V4, V5, V6];

const V6: &str = r#"
CREATE TABLE term_alias (
  id INTEGER PRIMARY KEY,
  term_id INTEGER NOT NULL REFERENCES term(id),
  source TEXT NOT NULL,
  UNIQUE (term_id, source)
);
"#;

const V5: &str = r#"
CREATE TABLE proposed_metadata (
  id INTEGER PRIMARY KEY,
  suggestion_id INTEGER NOT NULL REFERENCES suggestion(id),
  change TEXT NOT NULL,
  expected TEXT NOT NULL,
  before TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('pending', 'accepted', 'rejected', 'stale'))
);
CREATE INDEX proposed_metadata_suggestion ON proposed_metadata(suggestion_id);
"#;

const V1: &str = r#"
CREATE TABLE book_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE chapter (
  id INTEGER PRIMARY KEY,
  ord REAL NOT NULL,
  number INTEGER,
  title TEXT
);

CREATE TABLE pair (
  id INTEGER PRIMARY KEY,
  chapter_id INTEGER NOT NULL REFERENCES chapter(id),
  ord REAL NOT NULL,
  flagged TEXT
);

CREATE TABLE block (
  id INTEGER PRIMARY KEY,
  chapter_id INTEGER NOT NULL REFERENCES chapter(id),
  side TEXT NOT NULL CHECK (side IN ('source', 'translation')),
  ord REAL NOT NULL,
  pair_id INTEGER REFERENCES pair(id),
  text TEXT NOT NULL,
  hand_edited INTEGER NOT NULL DEFAULT 0,
  outdated INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX block_chapter ON block(chapter_id, side, ord);
CREATE INDEX block_pair ON block(pair_id);

CREATE TABLE term (
  id INTEGER PRIMARY KEY,
  source TEXT NOT NULL,
  rendering TEXT NOT NULL,
  kind TEXT,
  gender TEXT,
  note TEXT,
  status TEXT NOT NULL CHECK (status IN ('accepted', 'pending', 'dismissed')),
  first_chapter_id INTEGER REFERENCES chapter(id)
);

CREATE TABLE forbidden_rendering (
  id INTEGER PRIMARY KEY,
  term_id INTEGER NOT NULL REFERENCES term(id),
  rendering TEXT NOT NULL,
  UNIQUE (term_id, rendering)
);

CREATE TABLE style_rule (id INTEGER PRIMARY KEY, ord REAL NOT NULL, text TEXT NOT NULL);

CREATE TABLE form_of_address (
  id INTEGER PRIMARY KEY,
  speaker TEXT NOT NULL,
  listener TEXT NOT NULL,
  source_form TEXT NOT NULL,
  rendering TEXT NOT NULL
);

CREATE TABLE story_note (
  id INTEGER PRIMARY KEY,
  chapter_id INTEGER REFERENCES chapter(id),
  text TEXT NOT NULL
);

CREATE TABLE suggestion (
  id INTEGER PRIMARY KEY,
  chapter_id INTEGER NOT NULL REFERENCES chapter(id),
  block_id INTEGER NOT NULL REFERENCES block(id),
  quote TEXT NOT NULL,
  said TEXT NOT NULL,
  scope TEXT NOT NULL CHECK (scope IN ('here', 'everywhere', 'from_now_on')),
  is_rename INTEGER NOT NULL,
  source_expr TEXT,
  state TEXT NOT NULL CHECK (state IN ('working', 'ready', 'done')),
  created_at INTEGER NOT NULL
);

CREATE TABLE proposed_edit (
  id INTEGER PRIMARY KEY,
  suggestion_id INTEGER NOT NULL REFERENCES suggestion(id),
  block_id INTEGER NOT NULL REFERENCES block(id),
  before TEXT NOT NULL,
  after TEXT,
  origin TEXT NOT NULL CHECK (origin IN ('rename', 'model')),
  skip_reason TEXT,
  state TEXT NOT NULL CHECK (state IN ('pending', 'accepted', 'rejected', 'stale'))
);
CREATE INDEX proposed_edit_suggestion ON proposed_edit(suggestion_id);

CREATE TABLE revision (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  label TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  undone_by INTEGER REFERENCES revision(id)
);

CREATE TABLE revision_change (
  revision_id INTEGER NOT NULL REFERENCES revision(id),
  seq INTEGER NOT NULL,
  entity TEXT NOT NULL,
  entity_id INTEGER NOT NULL,
  before TEXT,
  after TEXT,
  PRIMARY KEY (revision_id, seq)
);

CREATE TABLE job (
  id INTEGER PRIMARY KEY,
  kind TEXT NOT NULL,
  parent_id INTEGER REFERENCES job(id),
  chapter_id INTEGER REFERENCES chapter(id),
  suggestion_id INTEGER REFERENCES suggestion(id),
  state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'checking', 'done', 'failed', 'paused')),
  input TEXT,
  partial TEXT NOT NULL DEFAULT '',
  attempts INTEGER NOT NULL DEFAULT 0,
  tokens_in INTEGER,
  tokens_out INTEGER,
  error TEXT,
  resume_after INTEGER,
  updated_at INTEGER NOT NULL
);
CREATE INDEX job_state ON job(state);

CREATE TABLE publish_state (
  chapter_id INTEGER PRIMARY KEY REFERENCES chapter(id),
  content_hash TEXT NOT NULL,
  published_at INTEGER NOT NULL
);
"#;

/// Indexes for reading one Chapter: without them a 1,200-Chapter Book scans every Pair each
/// time a Chapter opens.
const V2: &str = r#"
CREATE INDEX pair_chapter ON pair(chapter_id, ord);
CREATE INDEX job_chapter ON job(chapter_id, kind);
CREATE INDEX suggestion_chapter ON suggestion(chapter_id);
CREATE INDEX story_note_chapter ON story_note(chapter_id);
"#;

/// A Term the Translation may leave unnamed (`TermFields::may_omit`).
const V3: &str = r#"
ALTER TABLE term ADD COLUMN may_omit INTEGER NOT NULL DEFAULT 0;
"#;

/// Who asked for a Suggestion (`SuggestionKind`), and which model did a Job.
const V4: &str = r#"
ALTER TABLE suggestion ADD COLUMN kind TEXT NOT NULL DEFAULT 'reader';
ALTER TABLE job ADD COLUMN model TEXT;
"#;

pub(crate) fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    Ok(())
}

pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let latest = MIGRATIONS.len() as i64;
    if version > latest {
        return Err(Error::NewerSchema(version));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}
