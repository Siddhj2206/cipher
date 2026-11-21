//! Prompt building. Stable instructions first (prefix caching where a provider offers it),
//! then the Chapter's context, then the numbered source.
//!
//! Wording is provisional: `docs/plan.md` › Open questions has it settled during
//! milestone 4 by measuring on real Chapters.

use cipher_book::{Book, Chapter, Glossary, Term, TermStatus};
use cipher_text::matching::SourceForms;

use crate::Result;

/// Story Notes sent with a Chapter, in characters, newest kept.
pub(crate) const NOTES_BUDGET: usize = 1500;
/// Term notes are cut to this many characters: enough for gender and role, not a wiki.
const TERM_NOTE_CHARS: usize = 100;
/// Translated Blocks of the previous Chapter sent for continuity.
const PREVIOUS_TAIL: usize = 3;

pub(crate) const TRANSLATE: &str = "\
You translate Korean web fiction into natural, fluent English for readers of the genre.

Input paragraphs are numbered like `[12] text`. Write the translation in the same format:
- Start each entry with its number in square brackets, at the start of a line, and keep the order.
- Translate every number you are given, each exactly once.
- If two or more consecutive paragraphs read better as one, write one entry for them: `[12-13] text`.
- Write nothing else between entries: no headings, notes or commentary.
- Use the glossary renderings exactly. Entries marked provisional are new names; keep them consistent.
- Follow the style rules and forms of address.

After the last entry, write this trailer:
NEW_TERMS:
source | rendering | kind | gender
(one line per recurring name, place, title, item or skill that is not in the glossary; kind is one of name, place, title, item, skill, phrase; gender only for characters. Leave empty if none.)
NOTES:
(one or two sentences on what happened, for continuity in later chapters)";

pub(crate) const CONTINUE_NOTE: &str = "\
Translate only the numbered paragraphs under SOURCE. The paragraphs under CONTEXT are already translated; do not repeat them.";

pub(crate) const REPAIR: &str = "\
You fix specific problems in an English translation of Korean web fiction.

For each numbered paragraph you get the source, the current translation and the problem.
Reply with only the corrected translations, each starting with its number in square brackets at the start of a line, like `[12] text`. Change only what the problem needs. Use the glossary renderings exactly.";

pub(crate) const COMPACT: &str = "\
You keep running story notes for a translator of a long web novel. Rewrite the notes below into one summary of at most 250 words. Keep: who is who, alliances, titles, ongoing threads and anything a translator needs for consistency. Drop: blow-by-blow events that no longer matter. Use the names exactly as written. Reply with the summary only.";

/// Everything a Chapter's prompt says besides the source.
pub(crate) struct Context {
    /// Terms whose source form occurs in the Chapter: accepted, and pending as provisional.
    pub terms: Vec<Term>,
    pub text: String,
}

impl Context {
    pub(crate) fn gather(book: &Book, chapter: &Chapter, glossary: &Glossary) -> Result<Context> {
        let source: String = chapter
            .source
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let live: Vec<&Term> = glossary
            .terms
            .iter()
            .filter(|t| t.status != TermStatus::Dismissed)
            .collect();
        let sources = SourceForms::new(live.iter().map(|t| t.source_forms()));
        let terms: Vec<Term> = sources
            .terms_in(&source)
            .into_iter()
            .map(|i| live[i].clone())
            .collect();

        let mut out = String::new();
        if !glossary.style_rules.is_empty() {
            out.push_str("STYLE RULES:\n");
            for r in &glossary.style_rules {
                out.push_str(&format!("- {}\n", r.text));
            }
            out.push('\n');
        }
        if !glossary.forms_of_address.is_empty() {
            out.push_str("FORMS OF ADDRESS:\n");
            for a in &glossary.forms_of_address {
                let f = &a.fields;
                out.push_str(&format!(
                    "- {} to {}: {} = {}\n",
                    f.speaker, f.listener, f.source_form, f.rendering
                ));
            }
            out.push('\n');
        }
        if !terms.is_empty() {
            out.push_str("GLOSSARY:\n");
            for t in &terms {
                out.push_str(&term_line(t));
                out.push('\n');
            }
            out.push('\n');
        }
        let notes = story_notes(book, chapter)?;
        if !notes.is_empty() {
            out.push_str("STORY SO FAR:\n");
            out.push_str(&notes);
            out.push_str("\n\n");
        }
        let tail = previous_tail(book, chapter)?;
        if !tail.is_empty() {
            out.push_str("END OF THE PREVIOUS CHAPTER (already translated):\n");
            out.push_str(&tail);
            out.push_str("\n\n");
        }
        Ok(Context { terms, text: out })
    }
}

/// `마탑주 = Magic Tower Lord (title, male; Head of the Magic Tower…)`
pub(crate) fn term_line(t: &Term) -> String {
    let f = &t.fields;
    let sources = t.source_forms().collect::<Vec<_>>().join(" / ");
    let mut extra: Vec<String> = Vec::new();
    let mut tags: Vec<&str> = Vec::new();
    if t.status == TermStatus::Pending {
        tags.push("provisional");
    }
    if let Some(k) = f.kind.as_deref() {
        tags.push(k);
    }
    if let Some(g) = f.gender.as_deref() {
        tags.push(g);
    }
    if !tags.is_empty() {
        extra.push(tags.join(", "));
    }
    if let Some(note) = f.note.as_deref().filter(|n| !n.is_empty()) {
        extra.push(cut(note, TERM_NOTE_CHARS));
    }
    if !t.forbidden.is_empty() {
        extra.push(format!("never: {}", t.forbidden.join(", ")));
    }
    if extra.is_empty() {
        format!("{sources} = {}", f.rendering)
    } else {
        format!("{sources} = {} ({})", f.rendering, extra.join("; "))
    }
}

/// The compacted summary, then the newest per-Chapter notes before this Chapter, within
/// [`NOTES_BUDGET`].
fn story_notes(book: &Book, chapter: &Chapter) -> Result<String> {
    let order: Vec<_> = book.chapters()?.into_iter().map(|c| c.id).collect();
    let here = order
        .iter()
        .position(|&c| c == chapter.id)
        .unwrap_or(order.len());
    let notes = book.story_notes()?;
    let summary: Vec<&str> = notes
        .iter()
        .filter(|n| n.chapter.is_none())
        .map(|n| n.text.as_str())
        .collect();
    let mut recent: Vec<&str> = Vec::new();
    let mut used: usize = summary.iter().map(|s| s.chars().count()).sum();
    for n in notes.iter().rev() {
        let Some(c) = n.chapter else { continue };
        if order.iter().position(|&o| o == c).is_none_or(|p| p >= here) {
            continue;
        }
        used += n.text.chars().count();
        if used > NOTES_BUDGET {
            break;
        }
        recent.push(&n.text);
    }
    recent.reverse();
    Ok(summary
        .into_iter()
        .chain(recent)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn previous_tail(book: &Book, chapter: &Chapter) -> Result<String> {
    let order = book.chapters()?;
    let Some(here) = order.iter().position(|c| c.id == chapter.id) else {
        return Ok(String::new());
    };
    let Some(prev) = here.checked_sub(1).map(|i| order[i].id) else {
        return Ok(String::new());
    };
    let prev = book.chapter(prev)?;
    let start = prev.translation.len().saturating_sub(PREVIOUS_TAIL);
    Ok(prev.translation[start..]
        .iter()
        .map(|b| b.text.replace('\n', " "))
        .collect::<Vec<_>>()
        .join("\n\n"))
}

fn cut(s: &str, max: usize) -> String {
    let s = s.trim();
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cipher_book::{BookMeta, Change, ChapterId, TermFields};

    #[test]
    fn glossary_context_includes_the_survivor_when_only_an_alias_appears() {
        use cipher_book::TermId;
        let dir = tempfile::tempdir().unwrap();
        let mut book = Book::create(
            dir.path().join("aliases.cipher"),
            BookMeta {
                title: "Test".into(),
                source_lang: "ko".into(),
                target_lang: "en".into(),
            },
        )
        .unwrap();
        let id = |book: &mut Book, source: &str, rendering: &str| {
            TermId(
                book.apply(Change::UpsertTerm {
                    id: None,
                    fields: TermFields {
                        source: source.into(),
                        rendering: rendering.into(),
                        ..Default::default()
                    },
                    status: TermStatus::Accepted,
                    first_chapter: None,
                })
                .unwrap()
                .created[0],
            )
        };
        let keep = id(&mut book, "고요한 탑", "Quiet Tower");
        let other = id(&mut book, "고요의 탑", "Silent Spire");
        let merge = book.preview_term_merge(keep, other).unwrap();
        book.apply(Change::MergeTerms(Box::new(merge))).unwrap();
        let chapter = ChapterId(
            book.apply(Change::AddChapter {
                number: Some(1),
                title: None,
                source: vec!["고요의 탑이 나타났다.".into()],
            })
            .unwrap()
            .created[0],
        );
        let context = Context::gather(
            &book,
            &book.chapter(chapter).unwrap(),
            &book.glossary().unwrap(),
        )
        .unwrap();
        assert_eq!(
            context.terms.iter().map(|t| t.id).collect::<Vec<_>>(),
            [keep]
        );
        assert!(
            context
                .text
                .contains("고요한 탑 / 고요의 탑 = Quiet Tower (never: Silent Spire)")
        );
    }

    #[test]
    fn term_status_controls_prompt_context() {
        let dir = tempfile::tempdir().unwrap();
        let mut book = Book::create(
            dir.path().join("prompt.cipher"),
            BookMeta {
                title: "Test".into(),
                source_lang: "ko".into(),
                target_lang: "en".into(),
            },
        )
        .unwrap();
        let chapter = book
            .apply(Change::AddChapter {
                number: Some(1),
                title: None,
                source: vec!["탑".into()],
            })
            .unwrap();
        let chapter = book.chapter(ChapterId(chapter.created[0])).unwrap();
        let term = book
            .apply(Change::UpsertTerm {
                id: None,
                fields: TermFields {
                    source: "탑".into(),
                    rendering: "Tower".into(),
                    ..TermFields::default()
                },
                status: TermStatus::Pending,
                first_chapter: None,
            })
            .unwrap();
        let id = cipher_book::TermId(term.created[0]);
        assert!(
            Context::gather(&book, &chapter, &book.glossary().unwrap())
                .unwrap()
                .text
                .contains("Tower (provisional)")
        );
        book.apply(Change::SetTermStatus {
            term: id,
            status: TermStatus::Accepted,
        })
        .unwrap();
        let context = Context::gather(&book, &chapter, &book.glossary().unwrap()).unwrap();
        assert!(context.text.contains("탑 = Tower"));
        assert!(!context.text.contains("provisional"));
        book.apply(Change::SetTermStatus {
            term: id,
            status: TermStatus::Dismissed,
        })
        .unwrap();
        let context = Context::gather(&book, &chapter, &book.glossary().unwrap()).unwrap();
        assert!(context.terms.is_empty());
        assert!(!context.text.contains("Tower"));
    }
}
