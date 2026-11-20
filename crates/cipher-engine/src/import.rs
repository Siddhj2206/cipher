//! Importing a folder of markdown Chapters into a new Book.

use std::path::{Path, PathBuf};

use cipher_book::{
    Block, BlockId, Book, BookMeta, Change, ChapterId, NewPair, RevisionKind, TermFields,
    TermStatus,
};
use cipher_text::align::{Anchor, align};

use crate::{Error, Result};

/// What an import brought in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub chapters: usize,
    /// Chapters that came with an existing translation.
    pub translated: usize,
    pub terms: usize,
    /// Glossary entries without a source form (`og_term`), which can't become Terms.
    pub skipped_terms: usize,
    pub rules: usize,
}

/// Creates a Book at `out` from a folder laid out like `Test-Books/<name>`: `raw/<n>.md`,
/// optional `tl/<n>.md`, optional `glossary.json` (`[{term, og_term, definition, notes}]`)
/// and `style.md` (a rule per paragraph or list item, [`crate::style_rules`]). The title is
/// the folder's name; the languages are Korean to English. Fails if `out` exists or the
/// folder has no `raw/<n>.md`.
///
/// Existing translations come with no Pairs, so they are aligned locally
/// ([`cipher_text::align`], anchored on the glossary's names) and every Pair is flagged
/// `aligned`.
pub fn import(folder: &Path, out: &Path) -> Result<ImportReport> {
    let io = |what: &str, p: &Path, e: std::io::Error| {
        Error::Import(format!("cannot read {what} {}: {e}", p.display()))
    };
    let raw = folder.join("raw");
    let mut files: Vec<(i64, PathBuf)> = std::fs::read_dir(&raw)
        .map_err(|e| io("the folder", &raw, e))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter_map(|p| Some((p.file_stem()?.to_str()?.parse().ok()?, p)))
        .collect();
    if files.is_empty() {
        return Err(Error::Import(format!(
            "{} has no numbered chapters (raw/1.md, raw/2.md, …)",
            folder.display()
        )));
    }
    files.sort();

    let title = folder.file_name().map_or("Untitled".into(), |n| {
        n.to_string_lossy().replace(['-', '_'], " ")
    });
    let mut book = Book::create(
        out,
        BookMeta {
            title,
            source_lang: "ko".into(),
            target_lang: "en".into(),
        },
    )?;
    let mut report = ImportReport::default();
    let names = glossary_names(&folder.join("glossary.json"));
    let anchors: Vec<Anchor> = names
        .iter()
        .map(|(source, rendering)| Anchor { source, rendering })
        .collect();

    for (number, path) in files {
        let text = std::fs::read_to_string(&path).map_err(|e| io("chapter", &path, e))?;
        let mut source = cipher_text::blocks::split(&text);
        cipher_text::heading::take_heading(&mut source);
        let tl = folder.join("tl").join(path.file_name().unwrap_or_default());
        let mut translation = match std::fs::read_to_string(&tl) {
            Ok(text) => cipher_text::blocks::split(&text),
            Err(_) => Vec::new(),
        };
        let title = cipher_text::heading::take_heading(&mut translation).and_then(|h| h.title);
        let chapter = ChapterId(
            book.apply(Change::AddChapter {
                number: Some(number),
                title,
                source,
            })?
            .created[0],
        );
        report.chapters += 1;
        if translation.is_empty() {
            continue;
        }
        let source = book.chapter(chapter)?.source;
        book.apply(Change::SetTranslation {
            chapter,
            pairs: aligned_pairs(&source, translation, &anchors),
        })?;
        report.translated += 1;
    }

    let glossary = folder.join("glossary.json");
    if let Ok(json) = std::fs::read_to_string(&glossary) {
        let entries: Vec<serde_json::Value> = serde_json::from_str(&json)
            .map_err(|e| Error::Import(format!("{}: {e}", glossary.display())))?;
        let field = |e: &serde_json::Value, k: &str| {
            e.get(k)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        };
        let changes: Vec<Change> = entries
            .iter()
            .filter_map(|e| {
                let note = [field(e, "definition"), field(e, "notes")]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                Some(Change::UpsertTerm {
                    id: None,
                    fields: TermFields {
                        source: field(e, "og_term")?,
                        rendering: field(e, "term")?,
                        kind: None,
                        gender: None,
                        note: (!note.is_empty()).then_some(note),
                        may_omit: false,
                    },
                    status: TermStatus::Accepted,
                    first_chapter: None,
                })
            })
            .collect();
        report.terms = changes.len();
        report.skipped_terms = entries.len() - changes.len();
        if !changes.is_empty() {
            book.apply(Change::Group {
                kind: RevisionKind::Import,
                label: format!("Imported {} terms", report.terms),
                changes,
            })?;
        }
    }

    if let Ok(style) = std::fs::read_to_string(folder.join("style.md")) {
        let changes: Vec<Change> = crate::style_rules(&style)
            .into_iter()
            .map(|text| Change::AddStyleRule { text })
            .collect();
        report.rules = changes.len();
        if !changes.is_empty() {
            book.apply(Change::Group {
                kind: RevisionKind::Import,
                label: format!("Imported {} style rules", report.rules),
                changes,
            })?;
        }
    }
    Ok(report)
}

/// The glossary's (source, rendering) pairs, for anchoring the alignment. Empty when there
/// is no glossary or it doesn't parse; the import reports the parse error itself.
fn glossary_names(path: &Path) -> Vec<(String, String)> {
    let Ok(json) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let entries: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
    entries
        .iter()
        .filter_map(|e| {
            let f = |k: &str| {
                e.get(k)
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
            };
            Some((f("og_term")?, f("term")?))
        })
        .collect()
}

/// Pairs an existing translation with its source Blocks by [`align`]. Every source Block
/// ends up in a Pair: one the translator left out joins the Pair before it (or after it, at
/// the start), so an imported Chapter never reads as partly untranslated.
pub(crate) fn aligned_pairs(
    source: &[Block],
    translation: Vec<String>,
    anchors: &[Anchor],
) -> Vec<NewPair> {
    let texts: Vec<String> = source.iter().map(|b| b.text.clone()).collect();
    let groups = align(&texts, &translation, anchors);
    let mut pairs: Vec<NewPair> = Vec::new();
    let mut orphans: Vec<BlockId> = Vec::new();
    for g in groups {
        let ids = source[g.source.clone()].iter().map(|b| b.id);
        if g.translation.is_empty() {
            match pairs.last_mut() {
                Some(last) => last.source.extend(ids),
                None => orphans.extend(ids),
            }
            continue;
        }
        let mut ids: Vec<BlockId> = ids.collect();
        if !orphans.is_empty() {
            ids.splice(0..0, orphans.drain(..));
        }
        pairs.push(NewPair {
            source: ids,
            translation: translation[g.translation].to_vec(),
            flagged: Some(ALIGNED.into()),
        });
    }
    pairs
}

/// The flag on Pairs an import aligned locally.
pub const ALIGNED: &str = "aligned";
/// The flag on Pairs an import before alignment paired by position; [`realign_imported`]
/// replaces them.
const BY_POSITION: &str = "imported";

/// Chapters per Revision when re-pairing: about a second of writing each.
const BATCH: usize = 150;

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// Re-pairs every Chapter that an earlier import paired by position (all its Pairs flagged
/// `imported`, no Hand Edits), as one Revision the user can undo. Returns how many Chapters
/// changed. Run once when such a Book opens.
pub fn realign_imported(book: &mut Book) -> cipher_book::Result<usize> {
    let anchors_owned: Vec<(String, String)> = book
        .glossary()?
        .terms
        .iter()
        .flat_map(|t| {
            t.source_forms()
                .map(|source| (source.to_string(), t.fields.rendering.clone()))
        })
        .collect();
    let anchors: Vec<Anchor> = anchors_owned
        .iter()
        .map(|(source, rendering)| Anchor { source, rendering })
        .collect();
    let mut todo = Vec::new();
    for summary in book.chapters()? {
        let chapter = book.chapter(summary.id)?;
        let by_position = !chapter.pairs.is_empty()
            && chapter
                .pairs
                .iter()
                .all(|p| p.flagged.as_deref() == Some(BY_POSITION));
        if by_position && !chapter.translation.iter().any(|b| b.hand_edited) {
            todo.push(chapter);
        }
    }
    // Aligning is the slow part and needs no database: spread it over the cores.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let per = todo.len().div_ceil(threads).max(1);
    let anchors = &anchors;
    let changes: Vec<Change> = std::thread::scope(|scope| {
        let handles: Vec<_> = todo
            .chunks(per)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|chapter| Change::SetTranslation {
                            chapter: chapter.id,
                            pairs: aligned_pairs(
                                &chapter.source,
                                chapter.translation.iter().map(|b| b.text.clone()).collect(),
                                anchors,
                            ),
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("alignment does not panic"))
            .collect()
    });
    // In batches, so no one write holds the Book for long while a translation is saving.
    let n = changes.len();
    let batches = n.div_ceil(BATCH);
    let mut changes = changes.into_iter();
    for k in 1..=batches {
        let batch: Vec<Change> = changes.by_ref().take(BATCH).collect();
        let label = if batches == 1 {
            format!("Re-paired {}", plural(n, "imported chapter"))
        } else {
            format!("Re-paired imported chapters ({k} of {batches})")
        };
        book.apply(Change::Group {
            kind: RevisionKind::Import,
            label,
            changes: batch,
        })?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: i64, text: &str) -> Block {
        Block {
            id: BlockId(id),
            side: cipher_book::Side::Source,
            pair: None,
            text: text.into(),
            hand_edited: false,
            outdated: false,
        }
    }

    #[test]
    fn alignment_follows_names_and_leaves_no_source_block_out() {
        let src = [
            block(1, "테실리드가 웃었다."),
            block(2, "“가자.”"),
            block(3, "끼약!"),
            block(4, "힐데는 고개를 끄덕이며 성큼성큼 앞으로 걸어 나갔다."),
        ];
        // The translator merged nothing but dropped the sound effect.
        let tl = vec![
            "Tesselid laughed.".to_string(),
            "\"Let's go.\"".to_string(),
            "Hilde nodded and strode forward with long steps.".to_string(),
        ];
        let anchors = [
            Anchor {
                source: "테실리드",
                rendering: "Tesselid",
            },
            Anchor {
                source: "힐데",
                rendering: "Hilde",
            },
        ];
        let pairs = aligned_pairs(&src, tl, &anchors);
        let all: Vec<BlockId> = pairs.iter().flat_map(|p| p.source.clone()).collect();
        assert_eq!(all, [BlockId(1), BlockId(2), BlockId(3), BlockId(4)]);
        let hilde = pairs
            .iter()
            .find(|p| p.translation[0].starts_with("Hilde"))
            .unwrap();
        assert!(hilde.source.contains(&BlockId(4)));
        assert!(!hilde.source.contains(&BlockId(1)));
        assert!(pairs.iter().all(|p| p.flagged.as_deref() == Some(ALIGNED)));
    }

    #[test]
    fn realigning_replaces_position_pairs_once_and_can_be_undone() {
        let dir = tempfile::tempdir().unwrap();
        let mut book = Book::create(
            dir.path().join("b.cipher"),
            BookMeta {
                title: "t".into(),
                source_lang: "ko".into(),
                target_lang: "en".into(),
            },
        )
        .unwrap();
        let ch = ChapterId(
            book.apply(Change::AddChapter {
                number: Some(1),
                title: None,
                source: vec!["끼약!".into(), "테실리드가 웃었다.".into()],
            })
            .unwrap()
            .created[0],
        );
        let src: Vec<BlockId> = book
            .chapter(ch)
            .unwrap()
            .source
            .iter()
            .map(|b| b.id)
            .collect();
        // What an old import wrote: by position, so the name sits with the sound effect.
        book.apply(Change::SetTranslation {
            chapter: ch,
            pairs: vec![NewPair {
                source: src.clone(),
                translation: vec!["Tesselid laughed.".into()],
                flagged: Some(BY_POSITION.into()),
            }],
        })
        .unwrap();
        book.apply(Change::UpsertTerm {
            id: None,
            fields: TermFields {
                source: "테실리드".into(),
                rendering: "Tesselid".into(),
                ..Default::default()
            },
            status: TermStatus::Accepted,
            first_chapter: None,
        })
        .unwrap();
        assert_eq!(realign_imported(&mut book).unwrap(), 1);
        let c = book.chapter(ch).unwrap();
        assert!(
            c.pairs
                .iter()
                .all(|p| p.flagged.as_deref() == Some(ALIGNED))
        );
        assert_eq!(realign_imported(&mut book).unwrap(), 0, "only once");
    }

    #[test]
    fn a_folder_imports_chapters_translations_terms_and_rules() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("my-novel");
        std::fs::create_dir_all(f.join("raw")).unwrap();
        std::fs::create_dir_all(f.join("tl")).unwrap();
        std::fs::write(f.join("raw/2.md"), "2화\n\n둘째 날.\n\n바람.").unwrap();
        std::fs::write(f.join("raw/1.md"), "1화\n\n첫날.").unwrap();
        std::fs::write(f.join("raw/notes.md"), "not a chapter").unwrap();
        std::fs::write(f.join("tl/1.md"), "# Chapter 1: Dawn\n\nThe first day.").unwrap();
        std::fs::write(
            f.join("glossary.json"),
            r#"[{"term":"Tower","og_term":"탑","definition":"a tower"},{"term":"No source"}]"#,
        )
        .unwrap();
        std::fs::write(f.join("style.md"), "# Style\n- Keep honorifics\n").unwrap();

        let out = dir.path().join("b.cipher");
        let r = import(&f, &out).unwrap();
        assert_eq!(
            r,
            ImportReport {
                chapters: 2,
                translated: 1,
                terms: 1,
                skipped_terms: 1,
                rules: 1
            }
        );
        let book = Book::open(&out).unwrap();
        assert_eq!(book.meta().unwrap().title, "my novel");
        let chapters = book.chapters().unwrap();
        assert_eq!(chapters[0].number, Some(1));
        assert_eq!(chapters[1].blocks, 2);
        assert!(import(&f, &out).is_err(), "never overwrites a Book");
        assert!(import(dir.path(), &dir.path().join("c.cipher")).is_err());
    }
}
