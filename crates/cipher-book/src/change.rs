//! Every kind of edit to a Book, and how `apply` writes it.

use std::collections::HashSet;

use rusqlite::types::Value;
use rusqlite::{OptionalExtension, params};

use crate::revision::{Table, Writer, snapshot};
use crate::types::*;
use crate::{Error, Result};

/// One edit. `Book::apply` turns it into one Revision, so it is also one undo step.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    DeleteChapter {
        chapter: ChapterId,
    },
    /// Appends a Chapter, or, when `number` is set, inserts it in number order.
    /// Creates a Chapter id. `source` is already split into Blocks.
    AddChapter {
        number: Option<i64>,
        title: Option<String>,
        source: Vec<String>,
    },
    /// Replaces the Chapter's whole Translation and Pairing. Source Blocks left out of
    /// every Pair stay untranslated. A source Block may appear in only one Pair.
    /// Hand Edits are not yet preserved (milestone 9).
    SetTranslation {
        chapter: ChapterId,
        pairs: Vec<NewPair>,
    },
    /// Rewrites one Block. On the source side this marks the Pair's translated Blocks
    /// Outdated. `hand` marks a translated Block as a Hand Edit; pipeline repairs pass false.
    EditBlock {
        block: BlockId,
        text: String,
        hand: bool,
    },
    /// Sets or clears a Pair's flag.
    FlagPair {
        pair: PairId,
        reason: Option<String>,
    },
    /// Creates a Suggestion id.
    AddSuggestion(NewSuggestion),
    AddSuggestionWithMetadata {
        suggestion: NewSuggestion,
        changes: Vec<MetadataChange>,
    },
    DecideMetadata {
        decisions: Vec<(MetadataEditId, bool)>,
    },
    AddProposedEdits {
        suggestion: SuggestionId,
        edits: Vec<NewProposedEdit>,
    },
    SetSuggestionState {
        suggestion: SuggestionId,
        state: SuggestionState,
        source_expr: Option<String>,
    },
    /// Accepts (`true`) or rejects (`false`) Proposed Edits. An accepted edit whose Block
    /// no longer reads its `before` becomes Stale instead and the Block is left alone.
    /// A Suggestion whose edits are all decided becomes Done.
    DecideEdits {
        decisions: Vec<(ProposedEditId, bool)>,
    },
    /// Creates a Term when `id` is None. When an existing Term's rendering changes, the old
    /// rendering becomes a Forbidden Rendering and the new one stops being forbidden.
    UpsertTerm {
        id: Option<TermId>,
        fields: TermFields,
        status: TermStatus,
        first_chapter: Option<ChapterId>,
    },
    SetTermStatus {
        term: TermId,
        status: TermStatus,
    },
    /// Deletes the Term and its Forbidden Renderings.
    DeleteTerm {
        term: TermId,
    },
    /// No-op if already forbidden.
    AddForbidden {
        term: TermId,
        rendering: String,
    },
    RemoveForbidden {
        term: TermId,
        rendering: String,
    },
    AddStyleRule {
        text: String,
    },
    /// `None` deletes the rule.
    EditStyleRule {
        id: StyleRuleId,
        text: Option<String>,
    },
    AddFormOfAddress(AddressFields),
    /// `None` deletes the row.
    EditFormOfAddress {
        id: AddressId,
        fields: Option<AddressFields>,
    },
    AddStoryNote {
        chapter: Option<ChapterId>,
        text: String,
    },
    /// `None` deletes the note.
    EditStoryNote {
        id: StoryNoteId,
        text: Option<String>,
    },
    /// Replaces every Story Note with one compacted summary.
    CompactStoryNotes {
        summary: String,
    },
    /// Several Changes as one Revision, e.g. a Chapter's Translation with its new Pending
    /// Terms and Story Note.
    Group {
        kind: RevisionKind,
        label: String,
        changes: Vec<Change>,
    },
}

impl Change {
    fn kind(&self) -> RevisionKind {
        use Change::*;
        match self {
            AddChapter { .. } | DeleteChapter { .. } => RevisionKind::Chapter,
            SetTranslation { .. } | FlagPair { .. } => RevisionKind::Translation,
            EditBlock { hand: true, .. } => RevisionKind::HandEdit,
            EditBlock { hand: false, .. } => RevisionKind::Translation,
            AddSuggestion(_)
            | AddSuggestionWithMetadata { .. }
            | AddProposedEdits { .. }
            | SetSuggestionState { .. } => RevisionKind::Suggestion,
            DecideEdits { .. } | DecideMetadata { .. } => RevisionKind::Review,
            UpsertTerm { .. }
            | SetTermStatus { .. }
            | DeleteTerm { .. }
            | AddForbidden { .. }
            | RemoveForbidden { .. }
            | AddStyleRule { .. }
            | EditStyleRule { .. }
            | AddFormOfAddress(_)
            | EditFormOfAddress { .. } => RevisionKind::Glossary,
            AddStoryNote { .. } | EditStoryNote { .. } | CompactStoryNotes { .. } => {
                RevisionKind::Notes
            }
            Group { kind, .. } => *kind,
        }
    }
}

pub(crate) fn apply(tx: &rusqlite::Transaction<'_>, change: Change) -> Result<Applied> {
    let mut w = Writer::begin(tx, change.kind())?;
    let mut created = Vec::new();
    let label = write(&mut w, change, &mut created)?;
    let revision = w.finish(&label)?;
    Ok(Applied { revision, created })
}

pub(crate) fn metadata_snapshot(
    conn: &rusqlite::Connection,
    change: &MetadataChange,
) -> Result<String> {
    let rows = match change {
        MetadataChange::Term { term, .. } => {
            let mut rows = vec![snapshot(conn, Table::Term, term.0)?];
            let mut stmt = conn.prepare(
                "SELECT id FROM forbidden_rendering WHERE term_id = ?1 ORDER BY rendering, id",
            )?;
            let ids = stmt
                .query_map([term.0], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for id in ids {
                rows.push(snapshot(conn, Table::ForbiddenRendering, id)?);
            }
            rows
        }
        MetadataChange::StoryNote { note, .. } => vec![snapshot(conn, Table::StoryNote, note.0)?],
    };
    Ok(serde_json::to_string(&rows)?)
}

fn decide_metadata(
    w: &mut Writer<'_>,
    decisions: Vec<(MetadataEditId, bool)>,
    created: &mut Vec<i64>,
) -> Result<String> {
    let mut touched = Vec::new();
    let (mut accepted, mut rejected, mut stale) = (0, 0, 0);
    let mut accepted_labels = Vec::new();
    for (id, accept) in decisions {
        let (suggestion, payload, expected, state): (i64, String, String, String) = w.tx().query_row(
            "SELECT suggestion_id, change, expected, state FROM proposed_metadata WHERE id = ?1", [id.0],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).optional()?.ok_or(Error::NotFound("proposed_metadata", id.0))?;
        if state != "pending" && state != "stale" {
            continue;
        }
        let change: MetadataChange = serde_json::from_str(&payload)?;
        let state = if !accept {
            rejected += 1;
            EditState::Rejected
        } else if state == "stale" || metadata_snapshot(w.tx(), &change)? != expected {
            stale += 1;
            EditState::Stale
        } else {
            match change {
                MetadataChange::Term {
                    term,
                    fields,
                    forbidden,
                } => {
                    accepted_labels.push(format!("glossary change for {}", fields.source));
                    let status: String = w.tx().query_row(
                        "SELECT status FROM term WHERE id = ?1",
                        [term.0],
                        |r| r.get(0),
                    )?;
                    write(
                        w,
                        Change::UpsertTerm {
                            id: Some(term),
                            fields,
                            status: TermStatus::parse(&status)?,
                            first_chapter: None,
                        },
                        created,
                    )?;
                    for rendering in forbidden {
                        write(w, Change::AddForbidden { term, rendering }, created)?;
                    }
                }
                MetadataChange::StoryNote { note, text } => {
                    accepted_labels.push("story note change".into());
                    write(
                        w,
                        Change::EditStoryNote {
                            id: note,
                            text: Some(text),
                        },
                        created,
                    )?;
                }
            }
            accepted += 1;
            EditState::Accepted
        };
        w.update(
            Table::ProposedMetadata,
            id.0,
            &[("state", text(state.as_str()))],
        )?;
        touched.push(suggestion);
    }
    touched.sort_unstable();
    touched.dedup();
    for id in touched {
        let open: i64 = w.tx().query_row(
            "SELECT (SELECT COUNT(*) FROM proposed_edit WHERE suggestion_id = ?1 AND state = 'pending') + (SELECT COUNT(*) FROM proposed_metadata WHERE suggestion_id = ?1 AND state IN ('pending', 'stale'))", [id], |r| r.get(0),
        )?;
        if open == 0 {
            w.update(Table::Suggestion, id, &[("state", text("done"))])?;
        }
    }
    if accepted > 0 && rejected == 0 && stale == 0 {
        return Ok(format!("Accepted {}", accepted_labels.join(", ")));
    }
    Ok(format!(
        "Reviewed Book changes: {accepted} accepted, {rejected} rejected, {stale} stale"
    ))
}

fn text(s: impl Into<String>) -> Value {
    Value::Text(s.into())
}

fn opt(s: Option<String>) -> Value {
    s.map_or(Value::Null, Value::Text)
}

fn int(i: i64) -> Value {
    Value::Integer(i)
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// Writes one Change and returns its Revision label.
fn write(w: &mut Writer<'_>, change: Change, created: &mut Vec<i64>) -> Result<String> {
    match change {
        Change::AddSuggestionWithMetadata {
            suggestion,
            changes,
        } => {
            let label = write(w, Change::AddSuggestion(suggestion), created)?;
            let id = *created.last().expect("created Suggestion");
            for change in changes {
                let expected = metadata_snapshot(w.tx(), &change)?;
                let before: String = match &change {
                    MetadataChange::Term { term, .. } => w.tx().query_row(
                        "SELECT rendering FROM term WHERE id = ?1",
                        [term.0],
                        |r| r.get(0),
                    )?,
                    MetadataChange::StoryNote { note, .. } => w.tx().query_row(
                        "SELECT text FROM story_note WHERE id = ?1",
                        [note.0],
                        |r| r.get(0),
                    )?,
                };
                w.insert(
                    Table::ProposedMetadata,
                    &[
                        ("suggestion_id", int(id)),
                        ("change", text(serde_json::to_string(&change)?)),
                        ("expected", text(expected)),
                        ("before", text(before)),
                        ("state", text("pending")),
                    ],
                )?;
            }
            Ok(label)
        }
        Change::DecideMetadata { decisions } => decide_metadata(w, decisions, created),
        Change::DeleteChapter { chapter } => {
            let (number, title) = chapter_head(w, chapter)?;
            let jobs = "chapter_id = ?1 OR suggestion_id IN (SELECT id FROM suggestion WHERE chapter_id = ?1)";
            let active: bool = w.tx().query_row(
                &format!("SELECT EXISTS (SELECT 1 FROM job WHERE ({jobs}) AND state IN ('queued', 'running', 'checking', 'paused'))"),
                [chapter.0], |r| r.get(0),
            )?;
            if active {
                return Err(Error::Invalid(
                    "Cannot delete a chapter with an active job; stop its jobs first".into(),
                ));
            }
            w.tx()
                .execute(&format!("DELETE FROM job WHERE {jobs}"), [chapter.0])?;
            w.tx().execute(
                "DELETE FROM publish_state WHERE chapter_id = ?1",
                [chapter.0],
            )?;
            for (table, query) in [
                (
                    Table::ProposedMetadata,
                    "SELECT id FROM proposed_metadata WHERE suggestion_id IN (SELECT id FROM suggestion WHERE chapter_id = ?1) ORDER BY id",
                ),
                (
                    Table::ProposedEdit,
                    "SELECT id FROM proposed_edit WHERE suggestion_id IN (SELECT id FROM suggestion WHERE chapter_id = ?1) OR block_id IN (SELECT id FROM block WHERE chapter_id = ?1) ORDER BY id",
                ),
                (
                    Table::Suggestion,
                    "SELECT id FROM suggestion WHERE chapter_id = ?1 ORDER BY id",
                ),
                (
                    Table::Block,
                    "SELECT id FROM block WHERE chapter_id = ?1 ORDER BY id",
                ),
                (
                    Table::Pair,
                    "SELECT id FROM pair WHERE chapter_id = ?1 ORDER BY id",
                ),
            ] {
                for id in ids(w, query, chapter.0)? {
                    w.delete(table, id)?;
                }
            }
            for (table, column) in [
                (Table::StoryNote, "chapter_id"),
                (Table::Term, "first_chapter_id"),
            ] {
                for id in ids(
                    w,
                    &format!(
                        "SELECT id FROM {} WHERE {column} = ?1 ORDER BY id",
                        table.name()
                    ),
                    chapter.0,
                )? {
                    w.update(table, id, &[(column, Value::Null)])?;
                }
            }
            w.delete(Table::Chapter, chapter.0)?;
            let name = number.map_or_else(|| "Chapter".into(), |n| format!("Chapter {n}"));
            let name = title
                .filter(|t| !t.is_empty())
                .map_or(name.clone(), |t| format!("{name}: {t}"));
            Ok(format!("Delete {name}"))
        }
        Change::AddChapter {
            number,
            title,
            source,
        } => {
            let ord = chapter_ord(w, number)?;
            let id = w.insert(
                Table::Chapter,
                &[
                    ("ord", Value::Real(ord)),
                    ("number", number.map_or(Value::Null, int)),
                    ("title", opt(title.clone())),
                ],
            )?;
            for (i, block) in source.into_iter().enumerate() {
                w.insert(
                    Table::Block,
                    &[
                        ("chapter_id", int(id)),
                        ("side", text("source")),
                        ("ord", Value::Real(i as f64)),
                        ("text", text(block)),
                    ],
                )?;
            }
            created.push(id);
            Ok(format!("Added {}", chapter_name(number, title.as_deref())))
        }

        Change::SetTranslation { chapter, pairs } => {
            set_translation(w, chapter, pairs)?;
            let (number, title) = chapter_head(w, chapter)?;
            Ok(format!(
                "Translated {}",
                chapter_name(number, title.as_deref())
            ))
        }

        Change::EditBlock {
            block,
            text: new,
            hand,
        } => {
            let (side, pair): (String, Option<i64>) = w
                .tx()
                .query_row(
                    "SELECT side, pair_id FROM block WHERE id = ?1",
                    [block.0],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(Error::NotFound("block", block.0))?;
            let side = Side::parse(&side)?;
            let mut cols = vec![("text", text(new))];
            if side == Side::Translation && hand {
                cols.push(("hand_edited", int(1)));
            }
            if side == Side::Translation {
                cols.push(("outdated", int(0)));
            }
            w.update(Table::Block, block.0, &cols)?;
            if let (Side::Source, Some(pair)) = (side, pair) {
                for id in ids(
                    w,
                    "SELECT id FROM block WHERE pair_id = ?1 AND side = 'translation'",
                    pair,
                )? {
                    w.update(Table::Block, id, &[("outdated", int(1))])?;
                }
            }
            Ok(match (side, hand) {
                (Side::Source, _) => "Edited the source".into(),
                (_, true) => "Edited a paragraph".into(),
                _ => "Repaired a paragraph".into(),
            })
        }

        Change::FlagPair { pair, reason } => {
            let label = if reason.is_some() {
                "Flagged a pair"
            } else {
                "Cleared a flag"
            };
            w.update(Table::Pair, pair.0, &[("flagged", opt(reason))])?;
            Ok(label.into())
        }

        Change::AddSuggestion(s) => {
            if s.said.trim().is_empty() {
                return Err(Error::Invalid("a Suggestion needs the user's words".into()));
            }
            let id = w.insert(
                Table::Suggestion,
                &[
                    ("chapter_id", int(s.chapter.0)),
                    ("block_id", int(s.block.0)),
                    ("quote", text(s.quote.clone())),
                    ("said", text(s.said.clone())),
                    ("scope", text(s.scope.as_str())),
                    ("is_rename", int(i64::from(s.is_rename))),
                    ("source_expr", opt(s.source_expr)),
                    ("state", text(s.state.as_str())),
                    ("kind", text(s.kind.as_str())),
                    ("created_at", int(crate::now())),
                ],
            )?;
            insert_edits(w, SuggestionId(id), s.edits)?;
            created.push(id);
            Ok(match s.kind {
                SuggestionKind::Drift => {
                    format!("Fixing drift of \u{201c}{}\u{201d}", short(&s.quote))
                }
                SuggestionKind::Reader => format!("Suggested \u{201c}{}\u{201d}", short(&s.said)),
            })
        }

        Change::AddProposedEdits { suggestion, edits } => {
            let n = edits.len();
            insert_edits(w, suggestion, edits)?;
            Ok(format!("Proposed {}", plural(n, "edit")))
        }

        Change::SetSuggestionState {
            suggestion,
            state,
            source_expr,
        } => {
            let mut cols = vec![("state", text(state.as_str()))];
            if source_expr.is_some() {
                cols.push(("source_expr", opt(source_expr)));
            }
            w.update(Table::Suggestion, suggestion.0, &cols)?;
            Ok("Updated a suggestion".into())
        }

        Change::DecideEdits { decisions } => decide_edits(w, decisions),

        Change::UpsertTerm {
            id,
            fields,
            status,
            first_chapter,
        } => {
            if fields.source.trim().is_empty() || fields.rendering.trim().is_empty() {
                return Err(Error::Invalid(
                    "a Term needs a source form and a rendering".into(),
                ));
            }
            let mut cols = vec![
                ("source", text(fields.source.clone())),
                ("rendering", text(fields.rendering.clone())),
                ("kind", opt(fields.kind)),
                ("gender", opt(fields.gender)),
                ("note", opt(fields.note)),
                ("may_omit", int(fields.may_omit as i64)),
                ("status", text(status.as_str())),
            ];
            match id {
                None => {
                    cols.push((
                        "first_chapter_id",
                        first_chapter.map_or(Value::Null, |c| int(c.0)),
                    ));
                    let id = w.insert(Table::Term, &cols)?;
                    created.push(id);
                    Ok(format!("Added {}", fields.rendering))
                }
                Some(term) => {
                    let old: String = w
                        .tx()
                        .query_row("SELECT rendering FROM term WHERE id = ?1", [term.0], |r| {
                            r.get(0)
                        })
                        .optional()?
                        .ok_or(Error::NotFound("term", term.0))?;
                    if first_chapter.is_some() {
                        cols.push((
                            "first_chapter_id",
                            first_chapter.map_or(Value::Null, |c| int(c.0)),
                        ));
                    }
                    w.update(Table::Term, term.0, &cols)?;
                    if old != fields.rendering {
                        add_forbidden(w, term, &old)?;
                        remove_forbidden(w, term, &fields.rendering)?;
                        Ok(format!("Renamed {old} \u{2192} {}", fields.rendering))
                    } else {
                        Ok(format!("Edited {old}"))
                    }
                }
            }
        }

        Change::SetTermStatus { term, status } => {
            w.update(Table::Term, term.0, &[("status", text(status.as_str()))])?;
            let name = term_name(w, term)?;
            Ok(match status {
                TermStatus::Accepted => format!("Accepted {name}"),
                TermStatus::Dismissed => format!("Dismissed {name}"),
                TermStatus::Pending => format!("Reopened {name}"),
            })
        }

        Change::DeleteTerm { term } => {
            let name = term_name(w, term)?;
            for id in ids(
                w,
                "SELECT id FROM forbidden_rendering WHERE term_id = ?1",
                term.0,
            )? {
                w.delete(Table::ForbiddenRendering, id)?;
            }
            w.delete(Table::Term, term.0)?;
            Ok(format!("Deleted {name}"))
        }

        Change::AddForbidden { term, rendering } => {
            add_forbidden(w, term, &rendering)?;
            Ok(format!("Never \u{201c}{rendering}\u{201d}"))
        }

        Change::RemoveForbidden { term, rendering } => {
            remove_forbidden(w, term, &rendering)?;
            Ok(format!("Allowed \u{201c}{rendering}\u{201d}"))
        }

        Change::AddStyleRule { text: rule } => {
            let ord: f64 = w.tx().query_row(
                "SELECT COALESCE(MAX(ord), 0) + 1 FROM style_rule",
                [],
                |r| r.get(0),
            )?;
            let id = w.insert(
                Table::StyleRule,
                &[("ord", Value::Real(ord)), ("text", text(rule))],
            )?;
            created.push(id);
            Ok("Added a style rule".into())
        }

        Change::EditStyleRule { id, text: rule } => match rule {
            Some(rule) => {
                w.update(Table::StyleRule, id.0, &[("text", text(rule))])?;
                Ok("Edited a style rule".into())
            }
            None => {
                w.delete(Table::StyleRule, id.0)?;
                Ok("Deleted a style rule".into())
            }
        },

        Change::AddFormOfAddress(f) => {
            let id = w.insert(Table::FormOfAddress, &address_cols(f))?;
            created.push(id);
            Ok("Added a form of address".into())
        }

        Change::EditFormOfAddress { id, fields } => match fields {
            Some(f) => {
                w.update(Table::FormOfAddress, id.0, &address_cols(f))?;
                Ok("Edited a form of address".into())
            }
            None => {
                w.delete(Table::FormOfAddress, id.0)?;
                Ok("Deleted a form of address".into())
            }
        },

        Change::AddStoryNote {
            chapter,
            text: note,
        } => {
            let id = w.insert(
                Table::StoryNote,
                &[
                    ("chapter_id", chapter.map_or(Value::Null, |c| int(c.0))),
                    ("text", text(note)),
                ],
            )?;
            created.push(id);
            Ok("Added a story note".into())
        }

        Change::EditStoryNote { id, text: note } => match note {
            Some(note) => {
                w.update(Table::StoryNote, id.0, &[("text", text(note))])?;
                Ok("Edited a story note".into())
            }
            None => {
                w.delete(Table::StoryNote, id.0)?;
                Ok("Deleted a story note".into())
            }
        },

        Change::CompactStoryNotes { summary } => {
            for id in ids(w, "SELECT id FROM story_note WHERE ?1 = ?1", 0)? {
                w.delete(Table::StoryNote, id)?;
            }
            w.insert(
                Table::StoryNote,
                &[("chapter_id", Value::Null), ("text", text(summary))],
            )?;
            Ok("Compacted story notes".into())
        }

        Change::Group { label, changes, .. } => {
            for change in changes {
                write(w, change, created)?;
            }
            Ok(label)
        }
    }
}

fn set_translation(w: &mut Writer<'_>, chapter: ChapterId, pairs: Vec<NewPair>) -> Result<()> {
    let source: Vec<i64> = ids(
        w,
        "SELECT id FROM block WHERE chapter_id = ?1 AND side = 'source' ORDER BY ord",
        chapter.0,
    )?;
    if source.is_empty() && !pairs.is_empty() {
        return Err(Error::NotFound("chapter", chapter.0));
    }
    let position = |b: BlockId| source.iter().position(|&s| s == b.0);
    let mut seen = HashSet::new();
    for p in &pairs {
        if p.source.is_empty() || p.translation.is_empty() {
            return Err(Error::Invalid(
                "every Pair needs a Block on each side".into(),
            ));
        }
        let mut prev: Option<usize> = None;
        for &b in &p.source {
            let at = position(b).ok_or_else(|| {
                Error::Invalid(format!(
                    "block {} is not a source Block of this Chapter",
                    b.0
                ))
            })?;
            if prev.is_some_and(|p| at != p + 1) {
                return Err(Error::Invalid(
                    "a Pair's source Blocks must be contiguous".into(),
                ));
            }
            prev = Some(at);
            if !seen.insert(b) {
                return Err(Error::Invalid(format!("block {} is in two Pairs", b.0)));
            }
        }
    }

    // The old translated Blocks, in order. One whose text comes back unchanged keeps its id,
    // so Suggestions and Proposed Edits on it survive a re-pairing or a partial rewrite; a
    // Pair that comes back exactly as it was is left alone, so re-pairing writes little.
    let old: Vec<(i64, String, f64, Option<i64>)> = {
        let mut st = w.tx().prepare(
            "SELECT id, text, ord, pair_id FROM block WHERE chapter_id = ?1 AND side = 'translation' ORDER BY ord",
        )?;
        st.query_map([chapter.0], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    };
    let old_pairs: Vec<(i64, f64, Option<String>)> = {
        let mut st = w
            .tx()
            .prepare("SELECT id, ord, flagged FROM pair WHERE chapter_id = ?1")?;
        st.query_map([chapter.0], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let source_pair: std::collections::HashMap<i64, i64> = {
        let mut st = w.tx().prepare(
            "SELECT id, pair_id FROM block WHERE chapter_id = ?1 AND side = 'source' AND pair_id IS NOT NULL ORDER BY ord",
        )?;
        st.query_map([chapter.0], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let sources_of = |pair: i64| -> Vec<i64> {
        source
            .iter()
            .copied()
            .filter(|b| source_pair.get(b) == Some(&pair))
            .collect()
    };
    let mut paired_source: HashSet<i64> = source_pair.keys().copied().collect();

    let mut kept = HashSet::new();
    let mut kept_pairs = HashSet::new();
    let mut next_old = 0;
    let mut first_new: Option<i64> = None;
    let mut ord = 0.0;
    for (i, p) in pairs.into_iter().enumerate() {
        // Which old Blocks the translated texts are, looking a little ahead in the old order.
        let mut cursor = next_old;
        let reuse: Vec<Option<usize>> = p
            .translation
            .iter()
            .map(|t| {
                let k = old[cursor.min(old.len())..]
                    .iter()
                    .take(8)
                    .position(|o| o.1 == *t)
                    .map(|k| cursor + k);
                if let Some(k) = k {
                    cursor = k + 1;
                }
                k
            })
            .collect();
        next_old = cursor;
        let src: Vec<i64> = p.source.iter().map(|b| b.0).collect();
        for b in &src {
            paired_source.remove(b);
        }
        // The same Pair as before: same source Blocks, same translated Blocks.
        let same = src
            .first()
            .and_then(|b| source_pair.get(b))
            .copied()
            .filter(|op| !kept_pairs.contains(op))
            .filter(|&op| sources_of(op) == src)
            .filter(|&op| {
                let olds: Vec<usize> = old
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| o.3 == Some(op))
                    .map(|(k, _)| k)
                    .collect();
                reuse.iter().all(Option::is_some)
                    && reuse.iter().flatten().copied().collect::<Vec<_>>() == olds
            });
        let pair = match same {
            Some(op) => {
                kept_pairs.insert(op);
                let (_, o_ord, o_flag) = old_pairs.iter().find(|x| x.0 == op).cloned().unwrap();
                if o_ord != i as f64 || o_flag != p.flagged {
                    w.update(
                        Table::Pair,
                        op,
                        &[("ord", Value::Real(i as f64)), ("flagged", opt(p.flagged))],
                    )?;
                }
                op
            }
            None => {
                let pair = w.insert(
                    Table::Pair,
                    &[
                        ("chapter_id", int(chapter.0)),
                        ("ord", Value::Real(i as f64)),
                        ("flagged", opt(p.flagged)),
                    ],
                )?;
                for b in &src {
                    w.update(Table::Block, *b, &[("pair_id", int(pair))])?;
                }
                pair
            }
        };
        for (t, r) in p.translation.into_iter().zip(reuse) {
            let id = match r {
                Some(k) => {
                    let (id, _, o_ord, o_pair) = &old[k];
                    kept.insert(*id);
                    if *o_ord != ord || *o_pair != Some(pair) {
                        w.update(
                            Table::Block,
                            *id,
                            &[("ord", Value::Real(ord)), ("pair_id", int(pair))],
                        )?;
                    }
                    *id
                }
                None => w.insert(
                    Table::Block,
                    &[
                        ("chapter_id", int(chapter.0)),
                        ("side", text("translation")),
                        ("ord", Value::Real(ord)),
                        ("pair_id", int(pair)),
                        ("text", text(t)),
                    ],
                )?,
            };
            first_new.get_or_insert(id);
            ord += 1.0;
        }
    }

    // Source Blocks the new Pairing leaves out are untranslated.
    for id in paired_source {
        w.update(Table::Block, id, &[("pair_id", Value::Null)])?;
    }

    // Old Blocks that are gone: their Proposed Edits go with them, and a Suggestion made on
    // one moves to the Chapter's first translated Block, so it stays where it was asked.
    for (id, ..) in old.iter().filter(|(id, ..)| !kept.contains(id)) {
        for edit in ids(w, "SELECT id FROM proposed_edit WHERE block_id = ?1", *id)? {
            w.delete(Table::ProposedEdit, edit)?;
        }
        for sid in ids(w, "SELECT id FROM suggestion WHERE block_id = ?1", *id)? {
            match first_new {
                Some(to) => w.update(Table::Suggestion, sid, &[("block_id", int(to))])?,
                None => {
                    for edit in ids(
                        w,
                        "SELECT id FROM proposed_edit WHERE suggestion_id = ?1",
                        sid,
                    )? {
                        w.delete(Table::ProposedEdit, edit)?;
                    }
                    for edit in ids(
                        w,
                        "SELECT id FROM proposed_metadata WHERE suggestion_id = ?1",
                        sid,
                    )? {
                        w.delete(Table::ProposedMetadata, edit)?;
                    }
                    w.delete(Table::Suggestion, sid)?;
                }
            }
        }
        w.delete(Table::Block, *id)?;
    }
    for (id, ..) in old_pairs.iter().filter(|(id, ..)| !kept_pairs.contains(id)) {
        w.delete(Table::Pair, *id)?;
    }
    Ok(())
}

fn insert_edits(
    w: &mut Writer<'_>,
    suggestion: SuggestionId,
    edits: Vec<NewProposedEdit>,
) -> Result<()> {
    for e in edits {
        w.insert(
            Table::ProposedEdit,
            &[
                ("suggestion_id", int(suggestion.0)),
                ("block_id", int(e.block.0)),
                ("before", text(e.before)),
                ("after", opt(e.after)),
                ("origin", text(e.origin.as_str())),
                ("skip_reason", opt(e.skip_reason)),
                ("state", text("pending")),
            ],
        )?;
    }
    Ok(())
}

fn decide_edits(w: &mut Writer<'_>, decisions: Vec<(ProposedEditId, bool)>) -> Result<String> {
    let (mut accepted, mut rejected, mut stale) = (0, 0, 0);
    let mut touched = Vec::new();
    for (edit, accept) in decisions {
        let (suggestion, block, before, after): (i64, i64, String, Option<String>) = w
            .tx()
            .query_row(
                "SELECT suggestion_id, block_id, before, after FROM proposed_edit WHERE id = ?1",
                [edit.0],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .ok_or(Error::NotFound("proposed_edit", edit.0))?;
        touched.push(suggestion);
        let state = if !accept {
            rejected += 1;
            EditState::Rejected
        } else {
            let current: String =
                w.tx()
                    .query_row("SELECT text FROM block WHERE id = ?1", [block], |r| {
                        r.get(0)
                    })?;
            match after {
                Some(after) if current == before => {
                    w.update(Table::Block, block, &[("text", text(after))])?;
                    accepted += 1;
                    EditState::Accepted
                }
                // A skipped Block has nothing to accept; treat accepting it as dismissing it.
                None => {
                    rejected += 1;
                    EditState::Rejected
                }
                Some(_) => {
                    stale += 1;
                    EditState::Stale
                }
            }
        };
        w.update(
            Table::ProposedEdit,
            edit.0,
            &[("state", text(state.as_str()))],
        )?;
    }
    touched.sort_unstable();
    touched.dedup();
    for s in touched {
        let open: i64 = w.tx().query_row(
            "SELECT (SELECT COUNT(*) FROM proposed_edit WHERE suggestion_id = ?1 AND state = 'pending') + (SELECT COUNT(*) FROM proposed_metadata WHERE suggestion_id = ?1 AND state IN ('pending', 'stale'))",
            [s],
            |r| r.get(0),
        )?;
        if open == 0 {
            w.update(Table::Suggestion, s, &[("state", text("done"))])?;
        }
    }
    let mut parts = Vec::new();
    if accepted > 0 {
        parts.push(format!("Accepted {}", plural(accepted, "edit")));
    }
    if rejected > 0 {
        parts.push(format!("skipped {rejected}"));
    }
    if stale > 0 {
        parts.push(format!("{stale} stale"));
    }
    let mut label = parts.join(", ");
    if let Some(first) = label.get(..1) {
        label = first.to_uppercase() + &label[1..];
    }
    Ok(if label.is_empty() {
        "Reviewed".into()
    } else {
        label
    })
}

fn add_forbidden(w: &mut Writer<'_>, term: TermId, rendering: &str) -> Result<()> {
    let exists: Option<i64> = w
        .tx()
        .query_row(
            "SELECT id FROM forbidden_rendering WHERE term_id = ?1 AND rendering = ?2",
            params![term.0, rendering],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_none() {
        w.insert(
            Table::ForbiddenRendering,
            &[("term_id", int(term.0)), ("rendering", text(rendering))],
        )?;
    }
    Ok(())
}

fn remove_forbidden(w: &mut Writer<'_>, term: TermId, rendering: &str) -> Result<()> {
    let id: Option<i64> = w
        .tx()
        .query_row(
            "SELECT id FROM forbidden_rendering WHERE term_id = ?1 AND rendering = ?2",
            params![term.0, rendering],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = id {
        w.delete(Table::ForbiddenRendering, id)?;
    }
    Ok(())
}

fn address_cols(f: AddressFields) -> Vec<(&'static str, Value)> {
    vec![
        ("speaker", text(f.speaker)),
        ("listener", text(f.listener)),
        ("source_form", text(f.source_form)),
        ("rendering", text(f.rendering)),
    ]
}

/// Ord for a new Chapter: at the end, or between its numbered neighbours.
fn chapter_ord(w: &Writer<'_>, number: Option<i64>) -> Result<f64> {
    let tx = w.tx();
    let last: Option<f64> = tx.query_row("SELECT MAX(ord) FROM chapter", [], |r| r.get(0))?;
    let end = last.map_or(0.0, |o| o + 1.0);
    let Some(n) = number else { return Ok(end) };
    let next: Option<f64> =
        tx.query_row("SELECT MIN(ord) FROM chapter WHERE number > ?1", [n], |r| {
            r.get(0)
        })?;
    let Some(next) = next else { return Ok(end) };
    let prev: Option<f64> =
        tx.query_row("SELECT MAX(ord) FROM chapter WHERE ord < ?1", [next], |r| {
            r.get(0)
        })?;
    Ok(match prev {
        Some(prev) => (prev + next) / 2.0,
        None => next - 1.0,
    })
}

fn chapter_head(w: &Writer<'_>, chapter: ChapterId) -> Result<(Option<i64>, Option<String>)> {
    w.tx()
        .query_row(
            "SELECT number, title FROM chapter WHERE id = ?1",
            [chapter.0],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(Error::NotFound("chapter", chapter.0))
}

fn chapter_name(number: Option<i64>, title: Option<&str>) -> String {
    match (number, title) {
        (Some(n), _) => format!("chapter {n}"),
        (None, Some(t)) => t.to_string(),
        (None, None) => "a chapter".into(),
    }
}

fn term_name(w: &Writer<'_>, term: TermId) -> Result<String> {
    w.tx()
        .query_row("SELECT rendering FROM term WHERE id = ?1", [term.0], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or(Error::NotFound("term", term.0))
}

fn ids(w: &Writer<'_>, sql: &str, arg: i64) -> Result<Vec<i64>> {
    let mut stmt = w.tx().prepare_cached(sql)?;
    let rows = stmt.query_map([arg], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn short(s: &str) -> String {
    let s = s.trim();
    match s.char_indices().nth(40) {
        Some((i, _)) => format!("{}\u{2026}", &s[..i]),
        None => s.to_string(),
    }
}
