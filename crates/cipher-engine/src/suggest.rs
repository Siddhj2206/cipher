//! Applying a Suggestion (`docs/design/translation-pipeline.md` › Applying a Suggestion).
//!
//! Local work first: resolving the source expression, finding candidates and Renames cost
//! nothing. Only Blocks that need judgement go to the model, 12 Pairs per call. A one-off
//! Selection whose source can't be resolved locally gets one tiny call first.

use std::collections::{BTreeMap, BTreeSet};

use cipher_book::{
    Block, BlockId, Book, Change, ChapterId, EditOrigin, Job, JobKind, JobSpec, JobState,
    NewProposedEdit, NewSuggestion, RevisionKind, Scope, Side, Suggestion, SuggestionFilter,
    SuggestionId, SuggestionKind, SuggestionState, Term, TermFields, TermId, TermStatus,
};
use cipher_llm::{Request, Usage};
use cipher_text::rename::{self, RenameSpec};
use cipher_text::{markers, matching, resolve};
use serde::{Deserialize, Serialize};

use crate::prompt;
use crate::worker::{Outcome, Worker};
use crate::{Event, Result};

/// Pairs per model call: cheap models drop ids in larger batches.
const BATCH: usize = 12;

/// What the user selected: text inside one translated Block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub chapter: ChapterId,
    pub block: BlockId,
    pub text: String,
}

/// What a Suggestion would reach, for the fix box. Local and free.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reach {
    /// Translated Blocks the Suggestion would touch.
    pub blocks: usize,
    pub chapters: usize,
    /// The source expression, when known (a Term, or resolved locally).
    pub source_expr: Option<String>,
    /// The Term the Selection is a rendering (or Forbidden Rendering) of.
    pub term: Option<TermId>,
    /// How many of `blocks` contain the Selection itself, i.e. a Rename's exact replacements.
    pub exact: usize,
}

/// A Suggestion as the fix box sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestionRequest {
    pub selection: Selection,
    /// The user's words: the new wording for a Rename, else what is wrong.
    pub said: String,
    pub scope: Scope,
    pub rename: bool,
}

/// One translated Block with its Pair's source, from anywhere in the Book.
#[derive(Debug, Clone)]
struct Candidate {
    chapter: ChapterId,
    block: BlockId,
    translation: String,
    /// The translation contains the Selection.
    exact: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct BatchInput {
    blocks: Vec<i64>,
}

/// Every translated Block in the Book with its Pair's source text.
fn corpus(book: &Book) -> Result<Vec<(ChapterId, Block, String)>> {
    let mut out = Vec::new();
    for summary in book.chapters()? {
        let chapter = book.chapter(summary.id)?;
        for pair in &chapter.pairs {
            let source: Vec<&str> = chapter
                .blocks_in(pair.id, Side::Source)
                .map(|b| b.text.as_str())
                .collect();
            let source = source.join("\n\n");
            for block in chapter.blocks_in(pair.id, Side::Translation) {
                out.push((chapter.id, block.clone(), source.clone()));
            }
        }
    }
    Ok(out)
}

/// The Term whose rendering or Forbidden Rendering is the Selection.
fn term_for<'a>(terms: &'a [Term], selection: &str) -> Option<&'a Term> {
    let s = selection.trim().to_lowercase();
    terms
        .iter()
        .filter(|t| t.status != TermStatus::Dismissed)
        .find(|t| {
            t.fields.rendering.to_lowercase() == s
                || t.forbidden.iter().any(|f| f.to_lowercase() == s)
        })
}

struct Plan {
    source_expr: Option<String>,
    term: Option<TermId>,
    candidates: Vec<Candidate>,
    /// The source expression is needed but could not be resolved locally.
    unresolved: bool,
}

fn plan(book: &Book, selection: &Selection, scope: Scope) -> Result<Plan> {
    let contains = |t: &str| !matching::find_rendering(t, &selection.text).is_empty();
    if scope == Scope::Here {
        let chapter = book.chapter(selection.chapter)?;
        let candidates = chapter
            .translation
            .iter()
            .filter(|b| b.id == selection.block)
            .map(|b| Candidate {
                exact: contains(&b.text),
                chapter: chapter.id,
                block: b.id,
                translation: b.text.clone(),
            })
            .collect();
        return Ok(Plan {
            source_expr: None,
            term: None,
            candidates,
            unresolved: false,
        });
    }

    let corpus = corpus(book)?;
    let glossary = book.glossary()?;
    let term = term_for(&glossary.terms, &selection.text);
    let (hits, others): (Vec<_>, Vec<_>) = corpus.iter().partition(|(_, b, _)| contains(&b.text));
    let source_expr = match term {
        Some(t) => Some(t.fields.source.clone()),
        None => {
            let hit_sources: Vec<&str> = hits.iter().map(|(_, _, s)| s.as_str()).collect();
            let other_sources: Vec<&str> = others.iter().map(|(_, _, s)| s.as_str()).collect();
            resolve::resolve(&hit_sources, &other_sources).map(|c| c.text)
        }
    };
    let unresolved = source_expr.is_none();
    let candidates = corpus
        .into_iter()
        .filter_map(|(chapter, b, source)| {
            let exact = contains(&b.text);
            let by_source = source_expr
                .as_deref()
                .is_some_and(|e| !matching::find(&source, &[e]).is_empty());
            (exact || by_source).then_some(Candidate {
                chapter,
                block: b.id,
                translation: b.text,
                exact,
            })
        })
        .collect();
    Ok(Plan {
        source_expr,
        term: term.map(|t| t.id),
        candidates,
        unresolved,
    })
}

/// What a Suggestion would reach. Reads the whole Book; call it when the Selection or
/// Scope changes, not on every keystroke.
pub fn reach(book: &Book, selection: &Selection, scope: Scope) -> Result<Reach> {
    let plan = plan(book, selection, scope)?;
    let chapters: BTreeSet<ChapterId> = plan.candidates.iter().map(|c| c.chapter).collect();
    Ok(Reach {
        blocks: plan.candidates.len(),
        chapters: chapters.len(),
        exact: plan.candidates.iter().filter(|c| c.exact).count(),
        source_expr: plan.source_expr,
        term: plan.term,
    })
}

/// Records a Suggestion. A Rename's exact replacements become Proposed Edits at once;
/// everything that needs the model becomes Jobs. From now on also writes the Term (or a
/// Style Rule) and renames in Story Notes, in the same Revision.
pub fn suggest(book: &mut Book, req: SuggestionRequest) -> Result<SuggestionId> {
    let said = req.said.trim().to_string();
    let plan = plan(book, &req.selection, req.scope)?;
    let spec = RenameSpec::new(req.selection.text.trim(), said.clone()).with_derived_partials();

    let mut edits = Vec::new();
    let mut for_model = Vec::new();
    for c in &plan.candidates {
        if req.rename
            && c.exact
            && let Some(r) = rename::rename(&c.translation, &spec)
        {
            edits.push(NewProposedEdit {
                block: c.block,
                before: c.translation.clone(),
                after: Some(r.text),
                origin: EditOrigin::Rename,
                skip_reason: None,
            });
            continue;
        }
        for_model.push(c.block);
    }
    let needs_resolve = plan.unresolved && req.scope != Scope::Here;
    let working = !for_model.is_empty() || needs_resolve;

    let mut changes = vec![Change::AddSuggestion(NewSuggestion {
        chapter: req.selection.chapter,
        block: req.selection.block,
        quote: req.selection.text.clone(),
        said: said.clone(),
        scope: req.scope,
        is_rename: req.rename,
        source_expr: plan.source_expr.clone(),
        state: if working {
            SuggestionState::Working
        } else {
            SuggestionState::Ready
        },
        kind: SuggestionKind::Reader,
        edits,
    })];
    if req.scope == Scope::FromNowOn {
        changes.extend(from_now_on(book, &req, &plan, &spec)?);
    }
    let applied = book.apply(Change::Group {
        kind: RevisionKind::Suggestion,
        label: format!("Suggested \u{201c}{}\u{201d}", short(&said)),
        changes,
    })?;
    let id = SuggestionId(applied.created[0]);
    // A From now on Rename that created a Term: the old wording must not come back.
    if let (Scope::FromNowOn, true, Some(&term)) = (req.scope, req.rename, applied.created.get(1)) {
        book.apply(Change::AddForbidden {
            term: TermId(term),
            rendering: req.selection.text.trim().to_string(),
        })?;
    }

    if needs_resolve {
        book.enqueue(JobSpec {
            kind: JobKind::ResolveSource,
            parent: None,
            chapter: Some(req.selection.chapter),
            suggestion: Some(id),
            input: None,
        })?;
    }
    enqueue_batches(book, id, &for_model)?;
    Ok(id)
}

/// Fixes a Term's Drift in `chapters` as one Suggestion, reviewed like any other
/// (`docs/design/translation-pipeline.md` › Fixing Drift).
///
/// Only Pairs whose source names the Term are touched. A Forbidden Rendering there becomes
/// an exact replacement, with no model call. In a Chapter that never renders the Term, its
/// Blocks that leave the Term out go to the model, which may keep a pronoun where English
/// wants one. `Ok(None)` when there is nothing to fix.
pub fn fix_drift(
    book: &mut Book,
    term: TermId,
    chapters: &[ChapterId],
) -> Result<Option<SuggestionId>> {
    let glossary = book.glossary()?;
    let Some(t) = glossary.terms.iter().find(|t| t.id == term) else {
        return Err(cipher_book::Error::NotFound("term", term.0).into());
    };
    let source = t.fields.source.trim().to_string();
    let rendering = t.fields.rendering.trim().to_string();
    let low = rendering.to_lowercase();
    // A Forbidden Rendering inside the rendering itself ("Tower Lord" in "Magic Tower Lord")
    // can't be replaced word for word; those Blocks go to the model.
    let (local, judged): (Vec<&String>, Vec<&String>) = t
        .forbidden
        .iter()
        .filter(|f| !f.trim().is_empty() && f.to_lowercase() != low)
        .partition(|f| !low.contains(&f.to_lowercase()));

    let mut edits = Vec::new();
    let mut for_model = Vec::new();
    let mut anchor: Option<(ChapterId, BlockId)> = None;
    for &cid in chapters {
        let chapter = book.chapter(cid)?;
        let renders = |text: &str| !matching::find_rendering(text, &rendering).is_empty();
        let named_somewhere = chapter.translation.iter().any(|b| renders(&b.text));
        for pair in &chapter.pairs {
            let src: String = chapter
                .blocks_in(pair.id, Side::Source)
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if matching::find(&src, &[source.as_str()]).is_empty() {
                continue;
            }
            for b in chapter.blocks_in(pair.id, Side::Translation) {
                let mut text = b.text.clone();
                for f in &local {
                    if let Some(r) = rename::rename(&text, &RenameSpec::new(f.as_str(), &rendering))
                    {
                        text = r.text;
                    }
                }
                let has = |f: &&String| !matching::find_rendering(&b.text, f).is_empty();
                if text != b.text {
                    edits.push(NewProposedEdit {
                        block: b.id,
                        before: b.text.clone(),
                        after: Some(text),
                        origin: EditOrigin::Rename,
                        skip_reason: None,
                    });
                } else if judged.iter().any(has)
                    || (!t.fields.may_omit && !named_somewhere && !renders(&b.text))
                {
                    for_model.push(b.id);
                } else {
                    continue;
                }
                anchor.get_or_insert((cid, b.id));
            }
        }
    }
    let Some((chapter, block)) = anchor else {
        return Ok(None);
    };
    let said = format!("Name \u{201c}{rendering}\u{201d} wherever the source names {source}.");
    let applied = book.apply(Change::Group {
        kind: RevisionKind::Suggestion,
        label: format!("Fixed drift of \u{201c}{rendering}\u{201d}"),
        changes: vec![Change::AddSuggestion(NewSuggestion {
            chapter,
            block,
            quote: rendering.clone(),
            said,
            scope: Scope::Everywhere,
            is_rename: false,
            source_expr: Some(source),
            state: if for_model.is_empty() {
                SuggestionState::Ready
            } else {
                SuggestionState::Working
            },
            kind: SuggestionKind::Drift,
            edits,
        })],
    })?;
    let id = SuggestionId(applied.created[0]);
    enqueue_batches(book, id, &for_model)?;
    Ok(Some(id))
}

/// The Term or Style Rule a From now on Suggestion writes, and the Story Notes it renames.
fn from_now_on(
    book: &Book,
    req: &SuggestionRequest,
    plan: &Plan,
    spec: &RenameSpec,
) -> Result<Vec<Change>> {
    let mut out = Vec::new();
    let said = req.said.trim();
    if !req.rename {
        out.push(Change::AddStyleRule {
            text: format!("\u{201c}{}\u{201d}: {said}", req.selection.text.trim()),
        });
        return Ok(out);
    }
    let glossary = book.glossary()?;
    match plan
        .term
        .and_then(|id| glossary.terms.iter().find(|t| t.id == id))
    {
        Some(t) => out.push(Change::UpsertTerm {
            id: Some(t.id),
            fields: TermFields {
                rendering: said.to_string(),
                ..t.fields.clone()
            },
            status: TermStatus::Accepted,
            first_chapter: None,
        }),
        None => {
            if let Some(source) = &plan.source_expr {
                out.push(Change::UpsertTerm {
                    id: None,
                    fields: TermFields {
                        source: source.clone(),
                        rendering: said.to_string(),
                        ..TermFields::default()
                    },
                    status: TermStatus::Accepted,
                    first_chapter: None,
                });
            }
        }
    }
    for note in book.story_notes()? {
        if let Some(r) = rename::rename(&note.text, spec) {
            out.push(Change::EditStoryNote {
                id: note.id,
                text: Some(r.text),
            });
        }
    }
    Ok(out)
}

fn enqueue_batches(book: &mut Book, suggestion: SuggestionId, blocks: &[BlockId]) -> Result<()> {
    for chunk in blocks.chunks(BATCH) {
        let input = BatchInput {
            blocks: chunk.iter().map(|b| b.0).collect(),
        };
        book.enqueue(JobSpec {
            kind: JobKind::SuggestBatch,
            parent: None,
            chapter: None,
            suggestion: Some(suggestion),
            input: serde_json::to_string(&input).ok(),
        })?;
    }
    Ok(())
}

/// Blocks that already have a Proposed Edit or sit in a batch Job of this Suggestion.
fn planned_blocks(book: &Book, suggestion: &Suggestion) -> Result<BTreeSet<BlockId>> {
    let mut out: BTreeSet<BlockId> = suggestion.edits.iter().map(|e| e.block).collect();
    for job in book.jobs(false)? {
        if job.kind != JobKind::SuggestBatch || job.suggestion != Some(suggestion.id) {
            continue;
        }
        let input: BatchInput = job
            .input
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        out.extend(input.blocks.into_iter().map(BlockId));
    }
    Ok(out)
}

fn find_suggestion(book: &Book, id: SuggestionId) -> Result<Option<Suggestion>> {
    Ok(book
        .suggestions(SuggestionFilter::default())?
        .into_iter()
        .find(|s| s.id == id))
}

/// Marks the Suggestion Ready once no Job of its own is left to run.
fn finish_if_last(w: &mut Worker, job: &Job, emit: &mut dyn FnMut(Event)) -> Result<()> {
    let Some(id) = job.suggestion else {
        return Ok(());
    };
    let open = w.book.jobs(true)?.into_iter().any(|j| {
        j.id != job.id
            && j.suggestion == Some(id)
            && !matches!(j.state, JobState::Done | JobState::Failed)
    });
    if !open {
        let applied = w.book.apply(Change::SetSuggestionState {
            suggestion: id,
            state: SuggestionState::Ready,
            source_expr: None,
        })?;
        emit(Event::BookChanged {
            revision: applied.revision,
        });
    }
    Ok(())
}

const BATCH_PROMPT: &str = "\
You revise an English translation of Korean web fiction as a reader asked.

You get the reader's request, then numbered paragraphs, each with its source and current translation. For every number, reply with one line:
- `[n] the revised paragraph`, changing only what the request needs; or
- `[n] SKIP: reason` when the request does not apply to that paragraph.
Reply with nothing else. Use the glossary renderings exactly.";

const RESOLVE_PROMPT: &str = "\
You align a Korean source paragraph with its English translation. Reply with only the exact words of the source paragraph that the given English words translate, copied character for character, without trailing particles. Reply NONE if they have no counterpart.";

/// One batch of model-written Proposed Edits.
pub(crate) fn run_batch(
    w: &mut Worker,
    job: &mut Job,
    emit: &mut dyn FnMut(Event),
) -> Result<Outcome> {
    let Some(sid) = job.suggestion else {
        return Ok(Outcome::Failed(
            "a suggest_batch Job needs a Suggestion".into(),
        ));
    };
    let Some(suggestion) = find_suggestion(&w.book, sid)? else {
        return Ok(Outcome::Failed("the Suggestion is gone".into()));
    };
    let input: BatchInput = job
        .input
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    // Current text of each Block with its Pair's source.
    let wanted: BTreeSet<i64> = input.blocks.iter().copied().collect();
    let rows: BTreeMap<i64, (String, String)> = corpus(&w.book)?
        .into_iter()
        .filter(|(_, b, _)| wanted.contains(&b.id.0))
        .map(|(_, b, source)| (b.id.0, (source, b.text)))
        .collect();
    let blocks: Vec<(BlockId, String, String)> = input
        .blocks
        .iter()
        .filter_map(|id| {
            rows.get(id)
                .map(|(s, t)| (BlockId(*id), s.clone(), t.clone()))
        })
        .collect();
    if blocks.is_empty() {
        finish_if_last(w, job, emit)?;
        return Ok(Outcome::Done(Usage::default()));
    }

    let glossary = w.book.glossary()?;
    let all_source: String = blocks
        .iter()
        .map(|(_, s, _)| s.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let sources: Vec<&str> = glossary
        .terms
        .iter()
        .map(|t| t.fields.source.as_str())
        .collect();
    let terms: String = matching::terms_in(&all_source, &sources)
        .into_iter()
        .map(|i| prompt::term_line(&glossary.terms[i]) + "\n")
        .collect();

    let mut text = match (suggestion.kind, &suggestion.source_expr) {
        (SuggestionKind::Drift, Some(expr)) => format!(
            "REQUEST: The glossary renders {expr} as \u{201c}{q}\u{201d}. In each paragraph, \
             where the source names {expr} and the translation calls it something else or \
             leaves it out, name \u{201c}{q}\u{201d}. Keep a pronoun only where English reads \
             better with one. Reply SKIP when the paragraph already names it, or when {expr} \
             there is not this name (an ordinary word, another person).\n",
            q = suggestion.quote
        ),
        _ => {
            let mut t = format!(
                "REQUEST: The reader selected \u{201c}{}\u{201d} and said: {}\n",
                suggestion.quote, suggestion.said
            );
            if let Some(expr) = &suggestion.source_expr {
                t.push_str(&format!(
                    "The selected words translate the source expression: {expr}\n"
                ));
            }
            t
        }
    };
    if !terms.is_empty() {
        text.push_str(&format!("\nGLOSSARY:\n{terms}"));
    }
    for (i, (_, source, translation)) in blocks.iter().enumerate() {
        text.push_str(&format!(
            "\n[{}]\nSOURCE: {}\nTRANSLATION: {}\n",
            i + 1,
            source.replace('\n', " "),
            translation.replace('\n', " ")
        ));
    }
    let chars: usize = blocks.iter().map(|(_, _, t)| t.chars().count()).sum();
    let req = Request {
        instructions: BATCH_PROMPT.into(),
        input: text,
        max_output: (chars as u32 / 2).max(1000) + 2000,
        reasoning: w.models.reasoning,
    };
    let completion = match w.models.translate.complete(&req, &mut |_| {}) {
        Ok(c) => c,
        Err(e) => return Ok(Outcome::Model(e)),
    };

    let parsed = markers::parse(
        &completion.text,
        1..=blocks.len() as u32,
        completion.finished,
    );
    let mut answers: BTreeMap<u32, String> = BTreeMap::new();
    for p in parsed.pairs {
        if p.first == p.last {
            answers.entry(p.first).or_insert(p.text);
        }
    }
    let edits = blocks
        .iter()
        .enumerate()
        .map(|(i, (block, _, before))| {
            let answer = answers.get(&(i as u32 + 1)).map(|a| a.trim());
            let (after, skip) = match answer {
                None => (None, Some("no answer from the model".to_string())),
                Some(a) if a.to_uppercase().starts_with("SKIP") => {
                    let reason = a[4..].trim_start_matches([':', ' ', '-']).trim();
                    (
                        None,
                        Some(if reason.is_empty() {
                            "does not apply".into()
                        } else {
                            reason.to_string()
                        }),
                    )
                }
                Some(a) if a == before.trim() => (None, Some("already fine".into())),
                Some(a) => (Some(a.to_string()), None),
            };
            NewProposedEdit {
                block: *block,
                before: before.clone(),
                after,
                origin: EditOrigin::Model,
                skip_reason: skip,
            }
        })
        .collect();
    let applied = w.book.apply(Change::AddProposedEdits {
        suggestion: sid,
        edits,
    })?;
    emit(Event::BookChanged {
        revision: applied.revision,
    });
    finish_if_last(w, job, emit)?;
    Ok(Outcome::Done(completion.usage))
}

/// The tiny call for a one-off Selection: which source words does it translate? The answer
/// must be a literal substring of the source, or it is ignored. Then the Pairs that contain
/// that expression become model batches.
pub(crate) fn run_resolve(
    w: &mut Worker,
    job: &mut Job,
    emit: &mut dyn FnMut(Event),
) -> Result<Outcome> {
    let Some(sid) = job.suggestion else {
        return Ok(Outcome::Failed(
            "a resolve_source Job needs a Suggestion".into(),
        ));
    };
    let Some(suggestion) = find_suggestion(&w.book, sid)? else {
        return Ok(Outcome::Failed("the Suggestion is gone".into()));
    };
    let corpus = corpus(&w.book)?;
    let Some((_, block, source)) = corpus.iter().find(|(_, b, _)| b.id == suggestion.block) else {
        finish_if_last(w, job, emit)?;
        return Ok(Outcome::Done(Usage::default()));
    };
    let req = Request {
        instructions: RESOLVE_PROMPT.into(),
        input: format!(
            "SOURCE: {}\nTRANSLATION: {}\nENGLISH WORDS: {}",
            source.replace('\n', " "),
            block.text.replace('\n', " "),
            suggestion.quote
        ),
        max_output: 200,
        reasoning: w.models.reasoning,
    };
    let completion = match w.models.small().complete(&req, &mut |_| {}) {
        Ok(c) => c,
        Err(e) => return Ok(Outcome::Model(e)),
    };
    let answer = completion
        .text
        .trim()
        .trim_matches(['"', '\u{201c}', '\u{201d}', '`'])
        .trim()
        .to_string();
    if !answer.is_empty() && answer != "NONE" && source.contains(&answer) {
        let covered = planned_blocks(&w.book, &suggestion)?;
        let more: Vec<BlockId> = corpus
            .iter()
            .filter(|(_, b, s)| {
                !covered.contains(&b.id) && !matching::find(s, &[answer.as_str()]).is_empty()
            })
            .map(|(_, b, _)| b.id)
            .collect();
        let applied = w.book.apply(Change::SetSuggestionState {
            suggestion: sid,
            state: SuggestionState::Working,
            source_expr: Some(answer),
        })?;
        emit(Event::BookChanged {
            revision: applied.revision,
        });
        enqueue_batches(&mut w.book, sid, &more)?;
    }
    finish_if_last(w, job, emit)?;
    Ok(Outcome::Done(completion.usage))
}

fn short(s: &str) -> String {
    match s.char_indices().nth(40) {
        Some((i, _)) => format!("{}\u{2026}", &s[..i]),
        None => s.to_string(),
    }
}
