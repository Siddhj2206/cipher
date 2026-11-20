//! A Term's wordings across the Book, and choosing one (`docs/design/translation-pipeline.md`
//! › Drift). Local and free: the evidence is counted, the choice is an exact Rename.
//!
//! Cipher never decides which wording is right, or whether a wording is the same name: the
//! user marks the wordings that are, picks the one to keep, and reviews the Rename.

use std::collections::BTreeSet;

use cipher_book::{
    BlockId, Book, Change, ChapterId, EditOrigin, MetadataChange, NewProposedEdit, NewSuggestion,
    RevisionKind, Scope, Side, SuggestionId, SuggestionKind, SuggestionState, Term, TermFields,
    TermId, TermStatus,
};
use cipher_text::checks::PairText;
use cipher_text::matching::SourceForms;
use cipher_text::rename::{self, RenameSpec};
use cipher_text::wordings::{self, Query};

use crate::Result;

/// Example paragraphs kept per wording.
const EXAMPLES: usize = 2;

/// One Pair, as the evidence shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    pub chapter: ChapterId,
    pub number: Option<i64>,
    pub source: String,
    pub translation: String,
}

/// Where a wording is used.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// Pairs, by index into the Book's Pairs in reading order (for counting overlaps).
    pub pairs: Vec<u32>,
    /// First and last Chapter number it is used in.
    pub chapters: Option<(i64, i64)>,
    pub examples: Vec<Example>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wording {
    pub text: String,
    pub seen: Seen,
    /// Another Term whose rendering this is, contains or sits in.
    pub other_term: Option<TermId>,
    /// One of the Term's Forbidden Renderings: shown however rarely it is used, and already
    /// known to be this name.
    pub forbidden: bool,
}

/// What a Term is called where its source names it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Evidence {
    pub term: Option<TermId>,
    /// Pairs whose source names the Term.
    pub naming: usize,
    /// The Term's own rendering.
    pub rendered: Seen,
    /// Other wordings, most used first.
    pub wordings: Vec<Wording>,
    /// Pairs that name it neither way.
    pub neither: usize,
}

impl Evidence {
    /// Pairs a choice would change: those using a losing wording (the Term's rendering is
    /// wording `None`). `same` marks the wordings the user says are this name.
    pub fn changes(&self, winner: Option<usize>, same: &[bool]) -> usize {
        let mut out: BTreeSet<u32> = BTreeSet::new();
        if winner.is_some() {
            out.extend(&self.rendered.pairs);
        }
        for (i, w) in self.wordings.iter().enumerate() {
            if Some(i) != winner && same.get(i).copied().unwrap_or(false) {
                out.extend(&w.seen.pairs);
            }
        }
        out.len()
    }
}

/// One Pair of the Book, as read (inline markers removed).
struct BookPair {
    chapter: ChapterId,
    number: Option<i64>,
    source: String,
    translation: String,
    blocks: Vec<BlockId>,
}

/// Every translated Pair of the Book in reading order.
fn book_pairs(book: &Book) -> Result<Vec<BookPair>> {
    let mut out = Vec::new();
    for summary in book.chapters()? {
        if summary.blocks == 0 {
            continue;
        }
        let chapter = book.chapter(summary.id)?;
        if chapter.translation.is_empty() {
            continue;
        }
        for pair in &chapter.pairs {
            let side = |s: Side| {
                chapter
                    .blocks_in(pair.id, s)
                    .map(|b| cipher_text::blocks::render(&b.text).text)
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            out.push(BookPair {
                chapter: chapter.id,
                number: chapter.number,
                source: side(Side::Source),
                translation: side(Side::Translation),
                blocks: chapter
                    .blocks_in(pair.id, Side::Translation)
                    .map(|b| b.id)
                    .collect(),
            });
        }
    }
    Ok(out)
}

fn live_terms(book: &Book) -> Result<Vec<Term>> {
    Ok(book
        .glossary()?
        .terms
        .into_iter()
        .filter(|t| t.status != TermStatus::Dismissed)
        .collect())
}

/// The evidence for one Term. Reads the whole Book: a second or two on a long one, so call it
/// off the UI thread.
pub fn evidence(book: &Book, term: TermId) -> Result<Evidence> {
    Ok(gather(book, term)?.0)
}

/// The evidence, with the Book's Pairs and the indices of those naming it neither way.
fn gather(book: &Book, term: TermId) -> Result<(Evidence, Vec<BookPair>, Vec<usize>)> {
    let terms = live_terms(book)?;
    let Some(at) = terms.iter().position(|t| t.id == term) else {
        return Err(cipher_book::Error::NotFound("term", term.0).into());
    };
    let pairs = book_pairs(book)?;
    let texts: Vec<PairText> = pairs
        .iter()
        .map(|p| PairText {
            source: &p.source,
            translation: &p.translation,
        })
        .collect();
    let sources = SourceForms::new(terms.iter().map(Term::source_forms));
    let renderings: Vec<&str> = terms.iter().map(|t| t.fields.rendering.as_str()).collect();
    let ev = wordings::evidence(
        &texts,
        &Query {
            term: at,
            sources: &sources,
            renderings: &renderings,
        },
    );
    let seen = |idx: &[usize]| -> Seen {
        let numbers: Vec<i64> = idx.iter().filter_map(|&i| pairs[i].number).collect();
        // Examples from far apart: the first use, then the last.
        let mut picks: Vec<usize> = Vec::new();
        if let Some(&f) = idx.first() {
            picks.push(f);
        }
        if let Some(&l) = idx.last().filter(|&&l| Some(&l) != idx.first()) {
            picks.push(l);
        }
        Seen {
            pairs: idx.iter().map(|&i| i as u32).collect(),
            chapters: numbers
                .iter()
                .min()
                .zip(numbers.iter().max())
                .map(|(a, b)| (*a, *b)),
            examples: picks
                .into_iter()
                .take(EXAMPLES)
                .map(|i| Example {
                    chapter: pairs[i].chapter,
                    number: pairs[i].number,
                    source: pairs[i].source.clone(),
                    translation: pairs[i].translation.clone(),
                })
                .collect(),
        }
    };
    let mut wordings: Vec<Wording> = ev
        .wordings
        .iter()
        .map(|w| Wording {
            forbidden: terms[at]
                .forbidden
                .iter()
                .any(|f| f.eq_ignore_ascii_case(&w.text)),
            text: w.text.clone(),
            seen: seen(&w.pairs),
            other_term: w.other_term.map(|i| terms[i].id),
        })
        .collect();
    // Forbidden Renderings still in use are shown whatever their count.
    let mut neither = ev.neither.clone();
    for f in &terms[at].forbidden {
        if wordings.iter().any(|w| w.text.eq_ignore_ascii_case(f)) {
            continue;
        }
        let used: Vec<usize> = ev
            .naming
            .iter()
            .copied()
            .filter(|&i| {
                !cipher_text::matching::find_rendering(&pairs[i].translation, f).is_empty()
            })
            .collect();
        if used.is_empty() {
            continue;
        }
        neither.retain(|i| !used.contains(i));
        wordings.insert(
            0,
            Wording {
                text: f.clone(),
                seen: seen(&used),
                other_term: None,
                forbidden: true,
            },
        );
    }
    let evidence = Evidence {
        term: Some(term),
        naming: ev.naming.len(),
        rendered: seen(&ev.rendered),
        wordings,
        neither: neither.len(),
    };
    Ok((evidence, pairs, neither))
}

/// The opt-in model step: one Suggestion over the Pairs that name the Term neither way, sent
/// to the model in batches. Each answer must name the Term and change little else, or it is
/// recorded as left alone (`suggest::run_batch`). `Ok(None)` when there are none.
pub fn ask_model(book: &mut Book, term: TermId) -> Result<Option<SuggestionId>> {
    let (_, pairs, neither) = gather(book, term)?;
    let Some(t) = live_terms(book)?.into_iter().find(|t| t.id == term) else {
        return Err(cipher_book::Error::NotFound("term", term.0).into());
    };
    let blocks: Vec<(ChapterId, BlockId)> = neither
        .iter()
        .flat_map(|&i| {
            let chapter = pairs[i].chapter;
            pairs[i].blocks.iter().map(move |&b| (chapter, b))
        })
        .collect();
    let Some(&(chapter, block)) = blocks.first() else {
        return Ok(None);
    };
    let (source, rendering) = (t.fields.source.trim(), t.fields.rendering.trim());
    let applied = book.apply(Change::Group {
        kind: RevisionKind::Suggestion,
        label: format!("Asked the model to name \u{201c}{rendering}\u{201d}"),
        changes: vec![Change::AddSuggestion(NewSuggestion {
            chapter,
            block,
            quote: rendering.to_string(),
            said: format!("Name \u{201c}{rendering}\u{201d} where the source names {source}."),
            scope: Scope::Everywhere,
            is_rename: false,
            source_expr: Some(source.to_string()),
            state: SuggestionState::Working,
            kind: SuggestionKind::Drift,
            edits: Vec::new(),
        })],
    })?;
    let id = SuggestionId(applied.created[0]);
    let ids: Vec<BlockId> = blocks.into_iter().map(|(_, b)| b).collect();
    crate::suggest::enqueue_batches(book, id, &ids)?;
    Ok(Some(id))
}

/// Makes `winner` the Term's rendering and renames `same` (wordings the user marked as this
/// name, and the old rendering if it lost) to it, in Pairs whose source names the Term.
///
/// One Suggestion: the Term, Forbidden Renderings, Story Notes and exact replacements all
/// wait for explicit review. Another Term's rendering that
/// contains a losing wording is left alone. `Ok(None)` when nothing would change.
pub fn use_wording(
    book: &mut Book,
    term: TermId,
    winner: &str,
    same: &[String],
) -> Result<Option<SuggestionId>> {
    let terms = live_terms(book)?;
    let Some(t) = terms.iter().find(|t| t.id == term).cloned() else {
        return Err(cipher_book::Error::NotFound("term", term.0).into());
    };
    let winner = winner.trim().to_string();
    let rendering = t.fields.rendering.trim().to_string();
    let mut losers: Vec<String> = same
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case(&winner))
        .collect();
    if !rendering.eq_ignore_ascii_case(&winner) {
        losers.push(rendering.clone());
    }
    losers.sort_by_key(|l| std::cmp::Reverse(l.len()));
    losers.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    if losers.is_empty() {
        return Ok(None);
    }

    // The Blocks the Rename may touch: those in Pairs whose source names the Term.
    let source = t.fields.source.trim().to_string();
    let sources = SourceForms::new(terms.iter().map(Term::source_forms));
    let at = terms.iter().position(|o| o.id == term).unwrap_or(0);
    let mut blocks: Vec<(ChapterId, cipher_book::Block)> = Vec::new();
    for summary in book.chapters()? {
        let chapter = book.chapter(summary.id)?;
        for pair in &chapter.pairs {
            let src: String = chapter
                .blocks_in(pair.id, Side::Source)
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if sources.names_term(&src, at) {
                blocks.extend(
                    chapter
                        .blocks_in(pair.id, Side::Translation)
                        .map(|b| (chapter.id, b.clone())),
                );
            }
        }
    }
    // A plural wording is renamed by its singular, so both numbers are found and each keeps
    // its number; unless the singular never appears (`Outskirts`, `barrier arts`).
    let losers: Vec<String> = losers
        .into_iter()
        .map(|l| match rename::singular(&l) {
            Some(one) if blocks.iter().any(|(_, b)| occurs_alone(&b.text, &one)) => one,
            _ => l,
        })
        .collect();
    let lower: Vec<String> = losers.iter().map(|l| l.to_lowercase()).collect();
    let protect: Vec<String> = terms
        .iter()
        .filter(|o| o.id != term)
        .map(|o| o.fields.rendering.clone())
        .filter(|r| {
            let r = r.to_lowercase();
            r != winner.to_lowercase() && lower.iter().any(|l| r.contains(l.as_str()) && r != *l)
        })
        .collect();
    let mut spec = RenameSpec::new(losers[0].clone(), winner.clone()).for_term(protect);
    for l in &losers[1..] {
        spec = spec.with_partial(l.clone(), winner.clone());
    }

    let mut edits = Vec::new();
    let mut anchor = None;
    for (chapter, b) in &blocks {
        if let Some(r) = rename::rename(&b.text, &spec) {
            anchor.get_or_insert((*chapter, b.id));
            edits.push(NewProposedEdit {
                block: b.id,
                before: b.text.clone(),
                after: Some(r.text),
                origin: EditOrigin::Rename,
                skip_reason: None,
            });
        }
    }

    let mut metadata = Vec::new();
    let quote = losers
        .iter()
        .map(|l| format!("\u{201c}{l}\u{201d}"))
        .collect::<Vec<_>>()
        .join(", ");
    let anchor = anchor.or_else(|| blocks.first().map(|(ch, b)| (*ch, b.id)));
    let anchor = match anchor {
        Some(anchor) => Some(anchor),
        None => {
            let mut found = None;
            for summary in book.chapters()? {
                if let Some(b) = book.chapter(summary.id)?.source.first() {
                    found = Some((summary.id, b.id));
                    break;
                }
            }
            found
        }
    };
    let Some((chapter, block)) = anchor else {
        return Err(
            cipher_book::Error::Invalid("A Chapter is needed to review a wording".into()).into(),
        );
    };
    let mut forbidden = losers.clone();
    if rendering != winner && !forbidden.iter().any(|l| l.eq_ignore_ascii_case(&rendering)) {
        forbidden.push(rendering.clone());
    }
    if rendering != winner || !forbidden.is_empty() {
        metadata.push(MetadataChange::Term {
            term,
            fields: TermFields {
                rendering: winner.clone(),
                ..t.fields.clone()
            },
            forbidden,
        });
    }
    for note in book.story_notes()? {
        if let Some(r) = rename::rename(&note.text, &spec) {
            metadata.push(MetadataChange::StoryNote {
                note: note.id,
                text: r.text,
            });
        }
    }
    let applied = book.apply(Change::Group {
        kind: RevisionKind::Suggestion,
        label: format!("Review {quote} \u{2192} \u{201c}{winner}\u{201d}"),
        changes: vec![Change::AddSuggestionWithMetadata {
            suggestion: NewSuggestion {
                chapter,
                block,
                quote: losers.join(" / "),
                said: winner.clone(),
                scope: Scope::FromNowOn,
                is_rename: true,
                source_expr: Some(source.clone()),
                state: SuggestionState::Ready,
                kind: SuggestionKind::Reader,
                edits,
            },
            changes: metadata,
        }],
    })?;
    Ok(Some(SuggestionId(applied.created[0])))
}

/// `word` appears in `text` as whole words in any case, not followed by more letters (so not
/// as its plural).
fn occurs_alone(text: &str, word: &str) -> bool {
    let (text, word) = (text.to_lowercase(), word.to_lowercase());
    text.match_indices(&word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}
