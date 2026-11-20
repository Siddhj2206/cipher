//! Translating a Chapter: prepare → translate → check → repair → learn
//! (`docs/design/translation-pipeline.md`).
//!
//! Resumability: entries already accepted are kept as JSON in `job.input`
//! ([`Progress`]); `job.partial` holds only the current call's raw stream. After a crash
//! or a cut-off, the stream is parsed with its last entry dropped, and the next call asks
//! only for the numbers still missing.

use std::collections::BTreeSet;
use std::time::Instant;

use cipher_book::{
    BlockId, Change, Chapter, ChapterId, Job, NewPair, RevisionKind, Side, Term, TermFields,
    TermStatus,
};
use cipher_llm::{Effort, Request, Usage};
use cipher_text::checks::{self, CheckOptions, CheckTerm, Finding, PairText};
use cipher_text::markers;
use cipher_text::rename::{self, RenameSpec};
use serde::{Deserialize, Serialize};

use crate::prompt::{self, Context};
use crate::worker::{Outcome, Worker};
use crate::{Event, Result};

/// Calls per Chapter before giving up on missing numbers: the first plus continuations.
const MAX_ROUNDS: usize = 4;
/// Neighbouring translated Blocks shown around a re-requested range.
const CONTEXT_NEIGHBOURS: usize = 2;
/// How often the streamed partial is written to the Book.
const SAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct Progress {
    entries: Vec<Entry>,
    new_terms: Vec<(String, String, Option<String>, Option<String>)>,
    notes: Option<String>,
    tokens_in: u64,
    tokens_out: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    first: u32,
    last: u32,
    text: String,
}

impl Progress {
    fn covered(&self) -> BTreeSet<u32> {
        self.entries.iter().flat_map(|e| e.first..=e.last).collect()
    }

    /// Keeps entries that lie inside `wanted` and overlap nothing already accepted.
    fn absorb(&mut self, parsed: markers::Parsed, wanted: &BTreeSet<u32>) {
        let mut covered = self.covered();
        for p in parsed.pairs {
            let range: Vec<u32> = (p.first..=p.last).collect();
            if p.text.trim().is_empty()
                || !range
                    .iter()
                    .all(|n| wanted.contains(n) && !covered.contains(n))
            {
                continue;
            }
            covered.extend(range);
            self.entries.push(Entry {
                first: p.first,
                last: p.last,
                text: p.text.trim().to_string(),
            });
        }
        self.entries.sort_by_key(|e| e.first);
        for t in parsed.new_terms {
            if !self.new_terms.iter().any(|(s, ..)| *s == t.source) {
                self.new_terms
                    .push((t.source, t.rendering, t.kind, t.gender));
            }
        }
        if parsed.notes.is_some() {
            self.notes = parsed.notes;
        }
    }
}

pub(crate) fn run(w: &mut Worker, job: &mut Job, emit: &mut dyn FnMut(Event)) -> Result<Outcome> {
    let Some(chapter_id) = job.chapter else {
        return Ok(Outcome::Failed("a translate Job needs a Chapter".into()));
    };
    let chapter = w.book.chapter(chapter_id)?;
    let total = chapter.source.len() as u32;
    if total == 0 {
        return Ok(Outcome::Done(Usage::default()));
    }
    let glossary = w.book.glossary()?;
    let ctx = Context::gather(&w.book, &chapter, &glossary)?;

    // Prepare: resume what earlier attempts produced.
    let mut progress: Progress = job
        .input
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    let all: BTreeSet<u32> = (1..=total).collect();
    if !job.partial.is_empty() {
        let missing: BTreeSet<u32> = all.difference(&progress.covered()).copied().collect();
        progress.absorb(markers::parse(&job.partial, 1..=total, false), &missing);
        job.partial.clear();
        save(w, job, &progress)?;
    }

    // Translate, continuing until every number is covered.
    for _ in 0..MAX_ROUNDS {
        let wanted: BTreeSet<u32> = all.difference(&progress.covered()).copied().collect();
        if wanted.is_empty() {
            break; // a trailer lost to a cut-off is not worth a call of its own
        }
        let req = translate_request(&chapter, &ctx, &progress, &wanted, w.models.reasoning);
        let done_before = progress.covered().len();
        let accepted: Vec<(u32, u32, String)> = progress
            .entries
            .iter()
            .map(|e| (e.first, e.last, e.text.clone()))
            .collect();
        let mut last_save = Instant::now();
        let mut seen = 0;
        let book = &mut w.book;
        let result = w.models.translate.complete(&req, &mut |piece| {
            job.partial.push_str(piece);
            let entries =
                job.partial.matches("\n[").count() + usize::from(job.partial.starts_with('['));
            if entries != seen {
                seen = entries;
                emit(Event::Progress {
                    job: job.id,
                    chapter: chapter_id,
                    done: (done_before + entries.saturating_sub(1)).min(total as usize),
                    total: total as usize,
                });
                // The last entry may still be arriving: `parse` drops it when incomplete.
                let mut so_far = accepted.clone();
                so_far.extend(
                    markers::parse(&job.partial, 1..=total, false)
                        .pairs
                        .into_iter()
                        .filter(|p| !p.text.trim().is_empty())
                        .map(|p| (p.first, p.last, p.text)),
                );
                emit(Event::Streamed {
                    chapter: chapter_id,
                    entries: so_far,
                });
            }
            if last_save.elapsed() > SAVE_EVERY {
                last_save = Instant::now();
                let _ = book.save_job(job);
            }
        });
        match result {
            Ok(c) => {
                progress.tokens_in += c.usage.input;
                progress.tokens_out += c.usage.output;
                progress.absorb(markers::parse(&c.text, 1..=total, c.finished), &wanted);
                job.partial.clear();
                save(w, job, &progress)?;
            }
            Err(e) => {
                // Keep the streamed partial (already in `job.partial`) for the retry.
                save(w, job, &progress)?;
                return Ok(Outcome::Model(e));
            }
        }
    }

    let mut usage = Usage {
        input: progress.tokens_in,
        output: progress.tokens_out,
        ..Usage::default()
    };
    let missing: Vec<u32> = all.difference(&progress.covered()).copied().collect();

    // Write the Translation with what Learn found, as one Revision.
    let mut changes = vec![Change::SetTranslation {
        chapter: chapter_id,
        pairs: progress
            .entries
            .iter()
            .map(|e| NewPair {
                source: (e.first..=e.last)
                    .map(|n| chapter.source[n as usize - 1].id)
                    .collect(),
                translation: vec![e.text.clone()],
                flagged: None,
            })
            .collect(),
    }];
    changes.extend(learn_terms(
        &progress,
        &chapter,
        &glossary.terms,
        chapter_id,
    ));
    if let Some(notes) = progress
        .notes
        .as_deref()
        .map(|n| fix_forbidden(n, &glossary.terms))
        .filter(|n| !n.trim().is_empty())
    {
        changes.push(Change::AddStoryNote {
            chapter: Some(chapter_id),
            text: notes,
        });
    }
    let label = match chapter.number {
        Some(n) => format!("Translated chapter {n}"),
        None => "Translated a chapter".into(),
    };
    let applied = w.book.apply(Change::Group {
        kind: RevisionKind::Translation,
        label,
        changes,
    })?;
    emit(Event::BookChanged {
        revision: applied.revision,
    });

    // Check and repair.
    usage.add(&check_and_repair(w, chapter_id, &ctx.terms, emit)?);
    job.tokens_in = Some(usage.input as i64);
    job.tokens_out = Some(usage.output as i64);
    job.input = None;

    maybe_compact_notes(w)?;

    if !missing.is_empty() {
        return Ok(Outcome::Failed(format!(
            "the model left out {} after {MAX_ROUNDS} tries: {}",
            plural(missing.len(), "paragraph"),
            ranges(&missing)
        )));
    }
    Ok(Outcome::Done(usage))
}

fn save(w: &mut Worker, job: &mut Job, progress: &Progress) -> Result<()> {
    job.input = serde_json::to_string(progress).ok();
    job.tokens_in = Some(progress.tokens_in as i64);
    job.tokens_out = Some(progress.tokens_out as i64);
    w.book.save_job(job)?;
    Ok(())
}

fn translate_request(
    chapter: &Chapter,
    ctx: &Context,
    progress: &Progress,
    wanted: &BTreeSet<u32>,
    reasoning: Effort,
) -> Request {
    let lines: Vec<(u32, &str)> = wanted
        .iter()
        .map(|&n| (n, chapter.source[n as usize - 1].text.as_str()))
        .collect();
    let mut input = ctx.text.clone();
    let first_call = progress.entries.is_empty();
    if !first_call {
        input.push_str(prompt::CONTINUE_NOTE);
        input.push_str("\n\nCONTEXT:\n");
        let neighbours = neighbours(wanted, chapter.source.len() as u32);
        let context: Vec<String> = progress
            .entries
            .iter()
            .filter(|e| (e.first..=e.last).any(|n| neighbours.contains(&n)))
            .map(entry_line)
            .collect();
        input.push_str(&context.join("\n\n"));
        input.push_str("\n\nSOURCE:\n");
    }
    input.push_str(&markers::write(&lines));
    let chars: usize = lines.iter().map(|(_, t)| t.chars().count()).sum();
    Request {
        instructions: prompt::TRANSLATE.into(),
        input,
        max_output: max_output(chars, reasoning),
        reasoning,
    }
}

/// Output budget: Korean prose runs about 0.5 output tokens per source character
/// (2.4k tokens for 4.9k characters, measured), doubled for safety, plus the trailer
/// and headroom for hidden reasoning.
fn max_output(source_chars: usize, reasoning: Effort) -> u32 {
    let text = (source_chars as u32).saturating_add(1500);
    let thinking = match reasoning {
        Effort::Minimal => 0,
        Effort::Low => 4000,
        Effort::Medium | Effort::ProviderDefault => 8000,
        Effort::High => 16000,
    };
    text.max(2000) + thinking
}

/// Numbers within [`CONTEXT_NEIGHBOURS`] of a wanted number, but not wanted themselves.
fn neighbours(wanted: &BTreeSet<u32>, total: u32) -> BTreeSet<u32> {
    let k = CONTEXT_NEIGHBOURS as u32;
    wanted
        .iter()
        .flat_map(|&n| n.saturating_sub(k).max(1)..=(n + k).min(total))
        .filter(|n| !wanted.contains(n))
        .collect()
}

fn entry_line(e: &Entry) -> String {
    let text = e.text.replace('\n', " ");
    if e.first == e.last {
        format!("[{}] {text}", e.first)
    } else {
        format!("[{}-{}] {text}", e.first, e.last)
    }
}

/// NEW_TERMS → Pending Terms, skipping any whose source form the Chapter doesn't contain
/// or that overlaps a known Term by containment ("Duke Tesselid" against "Tesselid").
fn learn_terms(
    progress: &Progress,
    chapter: &Chapter,
    known: &[Term],
    id: ChapterId,
) -> Vec<Change> {
    let source: String = chapter
        .source
        .iter()
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let mut seen: Vec<(String, String)> = known
        .iter()
        .flat_map(|t| {
            t.source_forms()
                .map(|source| (source.to_string(), t.fields.rendering.to_lowercase()))
        })
        .collect();
    let mut out = Vec::new();
    for (src, rendering, kind, gender) in &progress.new_terms {
        let (src, rendering) = (src.trim(), rendering.trim());
        if src.is_empty() || rendering.is_empty() || !source.contains(src) {
            continue;
        }
        let low = rendering.to_lowercase();
        if seen
            .iter()
            .any(|(s, r)| s.contains(src) || src.contains(s.as_str()) || *r == low)
        {
            continue;
        }
        seen.push((src.to_string(), low));
        out.push(Change::UpsertTerm {
            id: None,
            fields: TermFields {
                source: src.into(),
                rendering: rendering.into(),
                kind: kind.clone().filter(|k| !k.is_empty()),
                gender: gender.clone().filter(|g| !g.is_empty()),
                note: None,
                may_omit: false,
            },
            status: TermStatus::Pending,
            first_chapter: Some(id),
        });
    }
    out
}

/// Replaces every Forbidden Rendering with its Term's rendering.
fn fix_forbidden(text: &str, terms: &[Term]) -> String {
    let mut text = text.to_string();
    for t in terms {
        for old in &t.forbidden {
            let spec =
                RenameSpec::new(old.clone(), t.fields.rendering.clone()).with_derived_partials();
            if let Some(r) = rename::rename(&text, &spec) {
                text = r.text;
            }
        }
    }
    text
}

fn check_terms(terms: &[Term]) -> Vec<CheckTerm> {
    terms
        .iter()
        .map(|t| CheckTerm {
            source: t.fields.source.clone(),
            aliases: t.aliases.clone(),
            rendering: t.fields.rendering.clone(),
            forbidden: t.forbidden.clone(),
            may_omit: t.fields.may_omit,
        })
        .collect()
}

/// The written Chapter as checkable Pairs: (translated Block id, source text, translation).
fn pair_texts(chapter: &Chapter) -> Vec<(BlockId, String, String)> {
    chapter
        .pairs
        .iter()
        .filter_map(|p| {
            let src: Vec<&str> = chapter
                .blocks_in(p.id, Side::Source)
                .map(|b| b.text.as_str())
                .collect();
            let mut tl = chapter.blocks_in(p.id, Side::Translation);
            let first = tl.next()?;
            Some((first.id, src.join("\n\n"), first.text.clone()))
        })
        .collect()
}

/// Check (local) and Repair: Forbidden Renderings are replaced locally as one undoable
/// Revision; leftover script and implausible lengths get one small call, retried once,
/// and Pairs that still fail are flagged. Missing names are left to Drift.
fn check_and_repair(
    w: &mut Worker,
    chapter_id: ChapterId,
    terms: &[Term],
    emit: &mut dyn FnMut(Event),
) -> Result<Usage> {
    let cterms = check_terms(terms);
    let options = CheckOptions::default();
    let mut usage = Usage::default();

    // Forbidden Renderings: zero-token local fix.
    let chapter = w.book.chapter(chapter_id)?;
    let pairs = pair_texts(&chapter);
    let mut fixes = Vec::new();
    for (block, src, tl) in &pairs {
        let hits = checks::forbidden_in_pair(
            &PairText {
                source: src,
                translation: tl,
            },
            &cterms,
        );
        if hits.is_empty() {
            continue;
        }
        let used: Vec<Term> = hits.iter().map(|h| terms[h.term_index].clone()).collect();
        let fixed = fix_forbidden(tl, &used);
        if fixed != *tl {
            fixes.push(Change::EditBlock {
                block: *block,
                text: fixed,
                hand: false,
            });
        }
    }
    if !fixes.is_empty() {
        let label = format!("Fixed {} with old names", plural(fixes.len(), "paragraph"));
        let applied = w.book.apply(Change::Group {
            kind: RevisionKind::Translation,
            label,
            changes: fixes,
        })?;
        emit(Event::BookChanged {
            revision: applied.revision,
        });
    }

    // Leftover script and length: up to two repair calls.
    for attempt in 0..2 {
        let chapter = w.book.chapter(chapter_id)?;
        let pairs = pair_texts(&chapter);
        let problems = repairable(&pairs, &cterms, &options);
        if problems.is_empty() {
            return Ok(usage);
        }
        if attempt == 1 {
            break;
        }
        let input = problems
            .iter()
            .map(|(i, problem)| {
                let (_, src, tl) = &pairs[*i];
                format!(
                    "[{}]\nSOURCE: {}\nTRANSLATION: {}\nPROBLEM: {problem}",
                    i + 1,
                    src.replace('\n', " "),
                    tl.replace('\n', " ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let glossary: String = terms.iter().map(|t| prompt::term_line(t) + "\n").collect();
        let req = Request {
            instructions: prompt::REPAIR.into(),
            input: if glossary.is_empty() {
                input
            } else {
                format!("GLOSSARY:\n{glossary}\n{input}")
            },
            max_output: 4000,
            reasoning: w.models.reasoning,
        };
        let completion = match w.models.small().complete(&req, &mut |_| {}) {
            Ok(c) => c,
            Err(_) => break, // a failed repair leaves the Pairs flagged; the Chapter stands
        };
        usage.add(&completion.usage);
        let n = pairs.len() as u32;
        let parsed = markers::parse(&completion.text, 1..=n, completion.finished);
        let wanted: BTreeSet<usize> = problems.iter().map(|(i, _)| *i).collect();
        let edits: Vec<Change> = parsed
            .pairs
            .into_iter()
            .filter(|p| p.first == p.last && wanted.contains(&(p.first as usize - 1)))
            .filter(|p| !p.text.trim().is_empty())
            .map(|p| Change::EditBlock {
                block: pairs[p.first as usize - 1].0,
                text: p.text.trim().to_string(),
                hand: false,
            })
            .collect();
        if !edits.is_empty() {
            let label = format!("Repaired {}", plural(edits.len(), "paragraph"));
            let applied = w.book.apply(Change::Group {
                kind: RevisionKind::Translation,
                label,
                changes: edits,
            })?;
            emit(Event::BookChanged {
                revision: applied.revision,
            });
        }
    }

    // Still failing: flag.
    let chapter = w.book.chapter(chapter_id)?;
    let pairs = pair_texts(&chapter);
    let flags: Vec<Change> = repairable(&pairs, &cterms, &options)
        .into_iter()
        .map(|(i, problem)| Change::FlagPair {
            pair: chapter.pairs[i].id,
            reason: Some(problem),
        })
        .collect();
    if !flags.is_empty() {
        let label = format!("Flagged {}", plural(flags.len(), "paragraph"));
        let applied = w.book.apply(Change::Group {
            kind: RevisionKind::Translation,
            label,
            changes: flags,
        })?;
        emit(Event::BookChanged {
            revision: applied.revision,
        });
    }
    Ok(usage)
}

/// Pairs with leftover source script or an implausible length, with the problem in words.
fn repairable(
    pairs: &[(BlockId, String, String)],
    terms: &[CheckTerm],
    options: &CheckOptions,
) -> Vec<(usize, String)> {
    let texts: Vec<PairText> = pairs
        .iter()
        .map(|(_, s, t)| PairText {
            source: s,
            translation: t,
        })
        .collect();
    let mut out: Vec<(usize, String)> = Vec::new();
    for f in checks::check_chapter(&texts, terms, options) {
        let (pair, problem) = match f {
            Finding::LeftoverScript { pair, run } => (
                pair,
                format!(
                    "untranslated text remains: \"{}\"",
                    &texts[pair].translation[run.range]
                ),
            ),
            Finding::LengthRatio { pair, problem, .. } => {
                (pair, format!("{problem:?}").to_lowercase())
            }
            _ => continue,
        };
        match out.iter_mut().find(|(p, _)| *p == pair) {
            Some((_, existing)) => {
                existing.push_str("; ");
                existing.push_str(&problem);
            }
            None => out.push((pair, problem)),
        }
    }
    out
}

/// Queues a compaction when Story Notes outgrow twice the prompt budget.
fn maybe_compact_notes(w: &mut Worker) -> Result<()> {
    let size: usize = w
        .book
        .story_notes()?
        .iter()
        .map(|n| n.text.chars().count())
        .sum();
    if size <= prompt::NOTES_BUDGET * 2 {
        return Ok(());
    }
    let pending = w.book.jobs(true)?.iter().any(|j| {
        j.kind == cipher_book::JobKind::CompactNotes && j.state != cipher_book::JobState::Failed
    });
    if !pending {
        w.book.enqueue(cipher_book::JobSpec {
            kind: cipher_book::JobKind::CompactNotes,
            parent: None,
            chapter: None,
            suggestion: None,
            input: None,
        })?;
    }
    Ok(())
}

/// Compacts all Story Notes into one summary with the small model.
pub(crate) fn compact_notes(w: &mut Worker) -> Result<Outcome> {
    let notes = w.book.story_notes()?;
    if notes.is_empty() {
        return Ok(Outcome::Done(Usage::default()));
    }
    let input: String = notes.iter().map(|n| format!("- {}\n", n.text)).collect();
    let req = Request {
        instructions: prompt::COMPACT.into(),
        input,
        max_output: 2000,
        reasoning: w.models.reasoning,
    };
    match w.models.small().complete(&req, &mut |_| {}) {
        Ok(c) if c.finished && !c.text.trim().is_empty() => {
            let glossary = w.book.glossary()?;
            let summary = fix_forbidden(c.text.trim(), &glossary.terms);
            w.book.apply(Change::CompactStoryNotes { summary })?;
            Ok(Outcome::Done(c.usage))
        }
        Ok(_) => Ok(Outcome::Failed("the summary was cut off or empty".into())),
        Err(e) => Ok(Outcome::Model(e)),
    }
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// `[1, 2, 3, 7]` → `1–3, 7`.
fn ranges(ns: &[u32]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < ns.len() {
        let start = ns[i];
        while i + 1 < ns.len() && ns[i + 1] == ns[i] + 1 {
            i += 1;
        }
        out.push(if ns[i] == start {
            start.to_string()
        } else {
            format!("{start}–{}", ns[i])
        });
        i += 1;
    }
    out.join(", ")
}
