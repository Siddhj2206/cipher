//! Revisions: every write records whole-row snapshots, so undo needs no per-table logic.
//!
//! A change row holds the full row before and after as a JSON object of its columns
//! (`before = NULL` means the row was created, `after = NULL` means it was deleted).
//! Undo walks a Revision's changes backwards and restores each `before`, but only while
//! the row still equals `after`; otherwise later work touched it. [`UndoMode::Whole`] then
//! reports the [`Conflict`]s and changes nothing; [`UndoMode::SkipConflicts`] reverts the
//! rest and reports what it skipped. An undo is itself a Revision, so undoing it redoes.

use rusqlite::types::{Value, ValueRef};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Map, Number, Value as Json};

use crate::types::{Conflict, RevisionId, RevisionKind, UndoMode, UndoOutcome};
use crate::{Error, Result, now};

pub(crate) fn preview(tx: &Transaction<'_>, id: RevisionId) -> Result<crate::UndoPreview> {
    let revision = tx
        .query_row(
            "SELECT kind, label, created_at, undone_by FROM revision WHERE id = ?1",
            [id.0],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(Error::NotFound("revision", id.0))?;
    let revision = crate::RevisionSummary {
        id,
        kind: RevisionKind::parse(&revision.0)?,
        label: revision.1,
        created_at: revision.2,
        undone_by: revision.3.map(RevisionId),
    };
    let mut chapters = std::collections::BTreeSet::new();
    let mut counts = std::collections::BTreeMap::new();
    let mut locations = Vec::new();
    {
        let mut stmt = tx.prepare("SELECT entity, entity_id, before, after FROM revision_change WHERE revision_id = ?1 ORDER BY seq")?;
        let rows = stmt.query_map([id.0], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        for row in rows {
            let (entity, row_id, before, after) = row?;
            let table = Table::from_name(&entity)?;
            *counts.entry(table.name()).or_insert(0) += 1;
            for row in [decode(before)?, decode(after)?].into_iter().flatten() {
                let chapter = if table == Table::Chapter {
                    Some(row_id)
                } else {
                    row.get("chapter_id").and_then(Json::as_i64)
                };
                let chapter = match (chapter, row.get("block_id").and_then(Json::as_i64)) {
                    (None, Some(block)) => tx
                        .query_row("SELECT chapter_id FROM block WHERE id = ?1", [block], |r| {
                            r.get(0)
                        })
                        .optional()?,
                    (chapter, _) => chapter,
                };
                let chapter = match (chapter, row.get("suggestion_id").and_then(Json::as_i64)) {
                    (None, Some(suggestion)) => tx
                        .query_row(
                            "SELECT chapter_id FROM suggestion WHERE id = ?1",
                            [suggestion],
                            |r| r.get(0),
                        )
                        .optional()?
                        .flatten(),
                    (chapter, _) => chapter,
                };
                if let Some(chapter) = chapter {
                    let chapter = crate::ChapterId(chapter);
                    chapters.insert(chapter);
                    let location = (
                        crate::Conflict {
                            entity: table.name(),
                            id: row_id,
                        },
                        chapter,
                    );
                    if !locations.contains(&location) {
                        locations.push(location);
                    }
                    if table == Table::ProposedMetadata
                        && let Some(payload) = row.get("change").and_then(Json::as_str)
                    {
                        let change: crate::MetadataChange = serde_json::from_str(payload)?;
                        let (entity, id) = match change {
                            crate::MetadataChange::Term { term, .. } => ("term", term.0),
                            crate::MetadataChange::StoryNote { note, .. } => ("story_note", note.0),
                        };
                        let location = (crate::Conflict { entity, id }, chapter);
                        if !locations.contains(&location) {
                            locations.push(location);
                        }
                    }
                }
            }
        }
    }
    let (conflicts, already_undone_by) = match undo(tx, id, UndoMode::SkipConflicts)? {
        UndoOutcome::Undone { skipped, .. } => (skipped, None),
        UndoOutcome::AlreadyUndone { by } => (Vec::new(), Some(by)),
        UndoOutcome::Conflict { conflicts } => (conflicts, None),
    };
    for conflict in &conflicts {
        if conflict.entity == "job" {
            let chapter: Option<i64> = tx.query_row(
                "SELECT chapter_id FROM job WHERE id = ?1",
                [conflict.id],
                |r| r.get(0),
            )?;
            if let Some(chapter) = chapter {
                let chapter = crate::ChapterId(chapter);
                chapters.insert(chapter);
                locations.push((conflict.clone(), chapter));
            }
        }
    }
    locations.retain(|(c, _)| conflicts.contains(c));
    Ok(crate::UndoPreview {
        revision,
        chapters: chapters.into_iter().collect(),
        changes: counts.into_iter().collect(),
        conflicts,
        conflict_chapters: locations,
        already_undone_by,
    })
}

/// The tables that Revisions cover. Jobs, Book metadata and publish state are operational
/// and sit outside history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Table {
    Chapter,
    Pair,
    Block,
    Term,
    ForbiddenRendering,
    StyleRule,
    FormOfAddress,
    StoryNote,
    Suggestion,
    ProposedEdit,
    ProposedMetadata,
}

impl Table {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Table::Chapter => "chapter",
            Table::Pair => "pair",
            Table::Block => "block",
            Table::Term => "term",
            Table::ForbiddenRendering => "forbidden_rendering",
            Table::StyleRule => "style_rule",
            Table::FormOfAddress => "form_of_address",
            Table::StoryNote => "story_note",
            Table::Suggestion => "suggestion",
            Table::ProposedEdit => "proposed_edit",
            Table::ProposedMetadata => "proposed_metadata",
        }
    }

    fn from_name(name: &str) -> Result<Table> {
        const ALL: [Table; 11] = [
            Table::Chapter,
            Table::Pair,
            Table::Block,
            Table::Term,
            Table::ForbiddenRendering,
            Table::StyleRule,
            Table::FormOfAddress,
            Table::StoryNote,
            Table::Suggestion,
            Table::ProposedEdit,
            Table::ProposedMetadata,
        ];
        ALL.into_iter()
            .find(|t| t.name() == name)
            .ok_or_else(|| Error::Corrupt(format!("unknown entity {name:?}")))
    }
}

type Row = Map<String, Json>;

/// Writes rows inside one transaction and records each write in the open Revision.
pub(crate) struct Writer<'a> {
    tx: &'a Transaction<'a>,
    revision: i64,
    seq: i64,
}

impl<'a> Writer<'a> {
    pub(crate) fn begin(tx: &'a Transaction<'a>, kind: RevisionKind) -> Result<Self> {
        tx.execute(
            "INSERT INTO revision (kind, label, created_at) VALUES (?1, '', ?2)",
            params![kind.as_str(), now()],
        )?;
        Ok(Writer {
            tx,
            revision: tx.last_insert_rowid(),
            seq: 0,
        })
    }

    pub(crate) fn tx(&self) -> &Transaction<'a> {
        self.tx
    }

    pub(crate) fn finish(self, label: &str) -> Result<RevisionId> {
        self.tx.execute(
            "UPDATE revision SET label = ?1 WHERE id = ?2",
            params![label, self.revision],
        )?;
        Ok(RevisionId(self.revision))
    }

    pub(crate) fn insert(&mut self, table: Table, cols: &[(&str, Value)]) -> Result<i64> {
        let names: Vec<&str> = cols.iter().map(|(c, _)| *c).collect();
        let marks: Vec<String> = (1..=cols.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            table.name(),
            names.join(", "),
            marks.join(", ")
        );
        self.tx.execute(
            &sql,
            rusqlite::params_from_iter(cols.iter().map(|(_, v)| v)),
        )?;
        let id = self.tx.last_insert_rowid();
        let after = snapshot(self.tx, table, id)?;
        self.record(table, id, None, after.as_ref())?;
        Ok(id)
    }

    /// Updates the given columns. Records nothing when no value actually changes.
    pub(crate) fn update(&mut self, table: Table, id: i64, cols: &[(&str, Value)]) -> Result<()> {
        let before = snapshot(self.tx, table, id)?.ok_or(Error::NotFound(table.name(), id))?;
        set_columns(
            self.tx,
            table,
            id,
            cols.iter().map(|(c, v)| (*c, v.clone())),
        )?;
        let after = snapshot(self.tx, table, id)?;
        if after.as_ref() != Some(&before) {
            self.record(table, id, Some(&before), after.as_ref())?;
        }
        Ok(())
    }

    pub(crate) fn delete(&mut self, table: Table, id: i64) -> Result<()> {
        let before = snapshot(self.tx, table, id)?.ok_or(Error::NotFound(table.name(), id))?;
        self.tx
            .execute(&format!("DELETE FROM {} WHERE id = ?1", table.name()), [id])?;
        self.record(table, id, Some(&before), None)
    }

    fn record(
        &mut self,
        table: Table,
        id: i64,
        before: Option<&Row>,
        after: Option<&Row>,
    ) -> Result<()> {
        self.seq += 1;
        let enc = |r: Option<&Row>| r.map(|r| Json::Object(r.clone()).to_string());
        self.tx.execute(
            "INSERT INTO revision_change (revision_id, seq, entity, entity_id, before, after)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                self.revision,
                self.seq,
                table.name(),
                id,
                enc(before),
                enc(after)
            ],
        )?;
        Ok(())
    }
}

/// Undoes `target` inside `tx`. On a `Conflict` outcome the caller must roll back.
pub(crate) fn undo(
    tx: &Transaction<'_>,
    target: RevisionId,
    mode: UndoMode,
) -> Result<UndoOutcome> {
    let (label, undone_by): (String, Option<i64>) = tx
        .query_row(
            "SELECT label, undone_by FROM revision WHERE id = ?1",
            [target.0],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(Error::NotFound("revision", target.0))?;
    if let Some(by) = undone_by {
        return Ok(UndoOutcome::AlreadyUndone { by: RevisionId(by) });
    }

    let changes: Vec<(String, i64, Option<String>, Option<String>)> = {
        let mut stmt = tx.prepare(
            "SELECT entity, entity_id, before, after FROM revision_change
             WHERE revision_id = ?1 ORDER BY seq DESC",
        )?;
        stmt.query_map([target.0], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    };

    let mut w = Writer::begin(tx, RevisionKind::Undo)?;
    let mut conflicts = Vec::new();
    for (entity, id, before, after) in changes {
        let table = Table::from_name(&entity)?;
        let before = decode(before)?;
        let after = decode(after)?;
        let current = snapshot(tx, table, id)?;
        if current != after {
            conflicts.push(Conflict {
                entity: table.name(),
                id,
            });
            continue;
        }
        let result = if table == Table::Chapter && before.is_none() && after.is_some() {
            let blocked = {
                let mut stmt = tx.prepare(
                    "SELECT id FROM job WHERE chapter_id = ?1 AND (state != 'queued' OR attempts != 0 OR EXISTS (SELECT 1 FROM job child WHERE child.parent_id = job.id))"
                )?;
                stmt.query_map([id], |r| r.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            if !blocked.is_empty() {
                conflicts.extend(blocked.into_iter().map(|id| Conflict { entity: "job", id }));
                continue;
            }
            tx.execute_batch("SAVEPOINT undo_chapter_jobs")?;
            tx.execute("DELETE FROM job WHERE chapter_id = ?1", [id])?;
            let result = w.delete(table, id);
            if result.is_err() {
                tx.execute_batch("ROLLBACK TO undo_chapter_jobs")?;
            }
            tx.execute_batch("RELEASE undo_chapter_jobs")?;
            result
        } else {
            match (&before, &after) {
                (None, Some(_)) => w.delete(table, id),
                (Some(row), None) => restore(&mut w, table, row),
                (Some(row), Some(_)) => w.update(
                    table,
                    id,
                    &row.iter()
                        .filter(|(k, _)| k.as_str() != "id")
                        .map(|(k, v)| (k.as_str(), to_value(v)))
                        .collect::<Vec<_>>(),
                ),
                (None, None) => Ok(()),
            }
        };
        match result {
            Ok(()) => {}
            // A foreign key stops the restore: something later depends on this row.
            Err(Error::Sqlite(rusqlite::Error::SqliteFailure(e, _)))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                conflicts.push(Conflict {
                    entity: table.name(),
                    id,
                });
            }
            Err(e) => return Err(e),
        }
    }
    if !conflicts.is_empty() && mode == UndoMode::Whole {
        return Ok(UndoOutcome::Conflict { conflicts });
    }
    let revision = w.finish(&format!("Undo: {label}"))?;
    tx.execute(
        "UPDATE revision SET undone_by = ?1 WHERE id = ?2",
        params![revision.0, target.0],
    )?;
    Ok(UndoOutcome::Undone {
        revision,
        skipped: conflicts,
    })
}

fn restore(w: &mut Writer<'_>, table: Table, row: &Row) -> Result<()> {
    let cols: Vec<(&str, Value)> = row.iter().map(|(k, v)| (k.as_str(), to_value(v))).collect();
    w.insert(table, &cols).map(|_| ())
}

fn set_columns<'c>(
    tx: &Transaction<'_>,
    table: Table,
    id: i64,
    cols: impl Iterator<Item = (&'c str, Value)>,
) -> Result<()> {
    let (names, mut values): (Vec<&str>, Vec<Value>) = cols.unzip();
    if names.is_empty() {
        return Ok(());
    }
    let sets: Vec<String> = names
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{c} = ?{}", i + 1))
        .collect();
    values.push(Value::Integer(id));
    let sql = format!(
        "UPDATE {} SET {} WHERE id = ?{}",
        table.name(),
        sets.join(", "),
        values.len()
    );
    tx.execute(&sql, rusqlite::params_from_iter(values))?;
    Ok(())
}

pub(crate) fn snapshot(tx: &rusqlite::Connection, table: Table, id: i64) -> Result<Option<Row>> {
    let mut stmt = tx.prepare_cached(&format!("SELECT * FROM {} WHERE id = ?1", table.name()))?;
    let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let row = stmt
        .query_row([id], |r| {
            let mut map = Map::new();
            for (i, name) in names.iter().enumerate() {
                map.insert(name.clone(), to_json(r.get_ref(i)?));
            }
            Ok(map)
        })
        .optional()?;
    Ok(row)
}

fn to_json(v: ValueRef<'_>) -> Json {
    match v {
        ValueRef::Null => Json::Null,
        ValueRef::Integer(i) => Json::Number(i.into()),
        ValueRef::Real(f) => Number::from_f64(f).map_or(Json::Null, Json::Number),
        ValueRef::Text(t) => Json::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(_) => Json::Null, // no history-covered column is a blob
    }
}

fn to_value(v: &Json) -> Value {
    match v {
        Json::Null => Value::Null,
        Json::Number(n) if n.is_f64() => Value::Real(n.as_f64().unwrap_or_default()),
        Json::Number(n) => Value::Integer(n.as_i64().unwrap_or_default()),
        Json::String(s) => Value::Text(s.clone()),
        Json::Bool(b) => Value::Integer(i64::from(*b)),
        other => Value::Text(other.to_string()),
    }
}

fn decode(s: Option<String>) -> Result<Option<Row>> {
    match s {
        None => Ok(None),
        Some(s) => match serde_json::from_str(&s)? {
            Json::Object(m) => Ok(Some(m)),
            _ => Err(Error::Corrupt("revision change is not an object".into())),
        },
    }
}
