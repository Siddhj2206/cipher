//! Reads. Each returns plain owned values; nothing borrows the connection.

use std::collections::HashMap;

use rusqlite::OptionalExtension;

use crate::types::*;
use crate::{Book, Error, Result};

impl Book {
    pub fn meta(&self) -> Result<BookMeta> {
        let get = |key: &str| -> Result<String> {
            Ok(self
                .conn
                .query_row("SELECT value FROM book_meta WHERE key = ?1", [key], |r| {
                    r.get(0)
                })
                .optional()?
                .unwrap_or_default())
        };
        Ok(BookMeta {
            title: get("title")?,
            source_lang: get("source_lang")?,
            target_lang: get("target_lang")?,
        })
    }

    /// Every Chapter in reading order.
    pub fn chapters(&self) -> Result<Vec<ChapterSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.number, c.title,
               (SELECT COUNT(*) FROM block b WHERE b.chapter_id = c.id AND b.side = 'source'),
               (SELECT COUNT(*) FROM block b WHERE b.chapter_id = c.id AND b.side = 'translation'),
               (SELECT COUNT(*) FROM block b WHERE b.chapter_id = c.id AND b.side = 'translation'
                  AND b.outdated = 1),
               (SELECT text FROM block b WHERE b.chapter_id = c.id AND b.side = 'source'
                  ORDER BY ord LIMIT 1),
               (SELECT state FROM job j WHERE j.chapter_id = c.id AND j.kind = 'translate'
                  ORDER BY j.id DESC LIMIT 1)
             FROM chapter c ORDER BY c.ord",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                ChapterId(r.get(0)?),
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, number, title, blocks, translated, outdated, first, job) = row?;
            let job = job.as_deref().map(JobState::parse).transpose()?;
            let state = match (translated > 0, job) {
                (false, _) if blocks == 0 => ChapterState::Empty,
                (_, Some(JobState::Running | JobState::Checking)) => ChapterState::Translating,
                (true, _) => ChapterState::Translated,
                (false, Some(JobState::Queued | JobState::Paused)) => ChapterState::Queued,
                (false, Some(JobState::Failed)) => ChapterState::Failed,
                (false, _) => ChapterState::Waiting,
            };
            out.push(ChapterSummary {
                id,
                number,
                title,
                state,
                blocks: blocks as usize,
                first_line: first.unwrap_or_default(),
                outdated: outdated as usize,
            });
        }
        Ok(out)
    }

    /// One Chapter with both sides and its Pairs.
    pub fn chapter(&self, id: ChapterId) -> Result<Chapter> {
        let (number, title) = self
            .conn
            .query_row(
                "SELECT number, title FROM chapter WHERE id = ?1",
                [id.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(Error::NotFound("chapter", id.0))?;
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, side, pair_id, text, hand_edited, outdated FROM block
             WHERE chapter_id = ?1 ORDER BY side, ord",
        )?;
        let blocks = stmt
            .query_map([id.0], |r| {
                Ok((
                    BlockId(r.get(0)?),
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?.map(PairId),
                    r.get::<_, String>(3)?,
                    r.get::<_, bool>(4)?,
                    r.get::<_, bool>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut source = Vec::new();
        let mut translation = Vec::new();
        for (id, side, pair, text, hand_edited, outdated) in blocks {
            let side = Side::parse(&side)?;
            let block = Block {
                id,
                side,
                pair,
                text,
                hand_edited,
                outdated,
            };
            match side {
                Side::Source => source.push(block),
                Side::Translation => translation.push(block),
            }
        }
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, flagged FROM pair WHERE chapter_id = ?1 ORDER BY ord")?;
        let pairs = stmt
            .query_map([id.0], |r| {
                Ok(Pair {
                    id: PairId(r.get(0)?),
                    flagged: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(Chapter {
            id,
            number,
            title,
            source,
            translation,
            pairs,
        })
    }

    /// Terms of every status, Style Rules and Forms of Address.
    pub fn glossary(&self) -> Result<Glossary> {
        let mut forbidden: HashMap<i64, Vec<String>> = HashMap::new();
        let mut stmt = self
            .conn
            .prepare("SELECT term_id, rendering FROM forbidden_rendering ORDER BY rendering")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
            let (term, rendering) = row?;
            forbidden.entry(term).or_default().push(rendering);
        }

        let mut stmt = self.conn.prepare(
            "SELECT id, source, rendering, kind, gender, note, status, first_chapter_id, may_omit
             FROM term ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                TermFields {
                    source: r.get(1)?,
                    rendering: r.get(2)?,
                    kind: r.get(3)?,
                    gender: r.get(4)?,
                    note: r.get(5)?,
                    may_omit: r.get(8)?,
                },
                r.get::<_, String>(6)?,
                r.get::<_, Option<i64>>(7)?,
            ))
        })?;
        let mut terms = Vec::new();
        for row in rows {
            let (id, fields, status, first) = row?;
            terms.push(Term {
                id: TermId(id),
                fields,
                status: TermStatus::parse(&status)?,
                first_chapter: first.map(ChapterId),
                forbidden: forbidden.remove(&id).unwrap_or_default(),
            });
        }

        let mut stmt = self
            .conn
            .prepare("SELECT id, text FROM style_rule ORDER BY ord")?;
        let style_rules = stmt
            .query_map([], |r| {
                Ok(StyleRule {
                    id: StyleRuleId(r.get(0)?),
                    text: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut stmt = self.conn.prepare(
            "SELECT id, speaker, listener, source_form, rendering FROM form_of_address ORDER BY id",
        )?;
        let forms_of_address = stmt
            .query_map([], |r| {
                Ok(FormOfAddress {
                    id: AddressId(r.get(0)?),
                    fields: AddressFields {
                        speaker: r.get(1)?,
                        listener: r.get(2)?,
                        source_form: r.get(3)?,
                        rendering: r.get(4)?,
                    },
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        Ok(Glossary {
            terms,
            style_rules,
            forms_of_address,
        })
    }

    /// Story Notes: the compacted summary first, then per-Chapter notes in reading order.
    pub fn story_notes(&self) -> Result<Vec<StoryNote>> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.chapter_id, n.text FROM story_note n
             LEFT JOIN chapter c ON c.id = n.chapter_id
             ORDER BY n.chapter_id IS NOT NULL, c.ord, n.id",
        )?;
        let notes = stmt
            .query_map([], |r| {
                Ok(StoryNote {
                    id: StoryNoteId(r.get(0)?),
                    chapter: r.get::<_, Option<i64>>(1)?.map(ChapterId),
                    text: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(notes)
    }

    /// Suggestions with their Proposed Edits, newest first.
    pub fn suggestions(&self, filter: SuggestionFilter) -> Result<Vec<Suggestion>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, chapter_id, block_id, quote, said, scope, is_rename, source_expr, state,
                    created_at, kind
             FROM suggestion
             WHERE (?1 IS NULL OR chapter_id = ?1) AND (?2 = 0 OR state != 'done')
             ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map((filter.chapter.map(|c| c.0), filter.open_only), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, bool>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, String>(10)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (
            id,
            chapter,
            block,
            quote,
            said,
            scope,
            is_rename,
            source_expr,
            state,
            created_at,
            kind,
        ) in rows
        {
            out.push(Suggestion {
                id: SuggestionId(id),
                chapter: ChapterId(chapter),
                block: BlockId(block),
                quote,
                said,
                scope: Scope::parse(&scope)?,
                is_rename,
                source_expr,
                state: SuggestionState::parse(&state)?,
                kind: SuggestionKind::parse(&kind)?,
                created_at,
                edits: self.edits(id)?,
            });
        }
        Ok(out)
    }

    fn edits(&self, suggestion: i64) -> Result<Vec<ProposedEdit>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT e.id, e.block_id, b.chapter_id, e.before, e.after, e.origin, e.skip_reason,
                    e.state, b.text
             FROM proposed_edit e JOIN block b ON b.id = e.block_id
             WHERE e.suggestion_id = ?1
             ORDER BY b.chapter_id, b.ord",
        )?;
        let rows = stmt
            .query_map([suggestion], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (id, block, chapter, before, after, origin, skip_reason, state, current) in rows {
            let mut state = EditState::parse(&state)?;
            if state == EditState::Pending && current != before {
                state = EditState::Stale;
            }
            out.push(ProposedEdit {
                id: ProposedEditId(id),
                block: BlockId(block),
                chapter: ChapterId(chapter),
                before,
                after,
                origin: EditOrigin::parse(&origin)?,
                skip_reason,
                state,
            });
        }
        Ok(out)
    }

    /// Recent Revisions, newest first.
    pub fn revisions(&self, limit: usize) -> Result<Vec<RevisionSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, label, created_at, undone_by FROM revision ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit as i64], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(id, kind, label, created_at, undone_by)| {
                Ok(RevisionSummary {
                    id: RevisionId(id),
                    kind: RevisionKind::parse(&kind)?,
                    label,
                    created_at,
                    undone_by: undone_by.map(RevisionId),
                })
            })
            .collect()
    }
}
