//! Book-wide search in the same plain text the reader sees.
use std::ops::Range;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use cipher_book::{BlockId, Book, ChapterId, Side};
use rune::{Key, KeyEvent, TextEdit, Tx};
use unicode_casefold::{Locale, UnicodeCaseFold, Variant};

use crate::{fix::work::Msg, library::Library, state::State};

pub const LIMIT: usize = 10_000;

#[derive(Clone, Default)]
pub struct Search {
    latest: Arc<AtomicU64>,
    generation: u64,
}

impl Search {
    fn next(&self) -> Self {
        Self {
            latest: self.latest.clone(),
            generation: self.latest.fetch_add(1, Ordering::Relaxed) + 1,
        }
    }

    fn cancelled(&self) -> bool {
        self.latest.load(Ordering::Relaxed) != self.generation
    }
}

#[derive(Clone)]
pub struct Hit {
    pub chapter: ChapterId,
    pub block: BlockId,
    pub side: Side,
    pub range: Range<usize>,
}

#[derive(Default)]
pub struct Results {
    pub hits: Vec<Hit>,
    pub more: bool,
}

pub struct Refresh {
    index: usize,
    hit: Option<Hit>,
}

impl Refresh {
    fn current(&self, results: &Results) -> usize {
        self.hit
            .as_ref()
            .and_then(|anchor| {
                results
                    .hits
                    .iter()
                    .enumerate()
                    .filter(|(_, hit)| {
                        hit.chapter == anchor.chapter
                            && hit.block == anchor.block
                            && hit.side == anchor.side
                    })
                    .min_by_key(|(_, hit)| hit.range.start.abs_diff(anchor.range.start))
                    .map(|(i, _)| i)
            })
            .unwrap_or_else(|| self.index.min(results.hits.len().saturating_sub(1)))
    }
}

pub struct Find {
    pub query: TextEdit,
    pub scope: Side,
    pub results: Results,
    pub current: usize,
    pub generation: u64,
    pub searching: bool,
    pub searched: String,
    pub search: Search,
    pub due: Option<f64>,
    pub book_generation: (u64, u64),
    pub refresh: Option<Refresh>,
}

impl Find {
    fn restart(&mut self) {
        self.generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        self.search = self.search.next();
        self.searching = !self.query.text.trim().is_empty();
        self.results = Results::default();
        self.current = 0;
        self.searched = self.query.text.clone();
        self.due = self.searching.then_some(f64::INFINITY);
        self.refresh = None;
    }

    fn schedule(&mut self, now: f64, book_generation: (u64, u64)) -> bool {
        let query_changed = self.searched != self.query.text;
        let changed = query_changed
            || (self.book_generation != book_generation && !self.query.text.trim().is_empty());
        self.book_generation = book_generation;
        if changed {
            let refresh = (!query_changed).then(|| {
                self.refresh.take().unwrap_or_else(|| Refresh {
                    index: self.current,
                    hit: self.results.hits.get(self.current).cloned(),
                })
            });
            self.restart();
            self.refresh = refresh;
        }
        if self.due.is_some_and(f64::is_infinite) {
            self.due = Some(now + 0.15);
        }
        changed
    }
}

impl Drop for Find {
    fn drop(&mut self) {
        self.search.latest.fetch_add(1, Ordering::Relaxed);
    }
}

fn fold(c: char) -> char {
    c.case_fold_with(Variant::Simple, Locale::NonTurkic)
        .next()
        .unwrap()
}

pub fn matches(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let needle: Vec<_> = query.chars().map(fold).collect();
    let chars: Vec<_> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= chars.len() {
        if chars[i..i + needle.len()]
            .iter()
            .map(|(_, c)| fold(*c))
            .eq(needle.iter().copied())
        {
            let end = chars.get(i + needle.len()).map_or(text.len(), |x| x.0);
            out.push(chars[i].0..end);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

impl Library {
    pub fn find_job(
        &self,
        query: String,
        side: Side,
        search: Search,
    ) -> impl FnOnce() -> cipher_book::Result<Results> + Send + 'static {
        let path = self.path().to_path_buf();
        let ids: Vec<_> = self.chapters.iter().map(|c| c.id).collect();
        move || {
            let mut out = Results::default();
            if query.trim().is_empty() || search.cancelled() {
                return Ok(out);
            }
            let book = Book::open(path)?;
            for chapter in ids {
                if search.cancelled() {
                    return Ok(Results::default());
                }
                let ch = book.chapter(chapter)?;
                let blocks = match side {
                    Side::Source => ch.source,
                    Side::Translation => ch.translation,
                };
                for b in blocks {
                    for range in matches(&crate::library::shown(&b.text), &query) {
                        if out.hits.len() == LIMIT {
                            out.more = true;
                            return Ok(out);
                        }
                        out.hits.push(Hit {
                            chapter,
                            block: b.id,
                            side,
                            range,
                        });
                    }
                }
            }
            Ok(out)
        }
    }
}

static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl State {
    pub fn open_find(&mut self, tx: &mut Tx) {
        self.palette = None;
        let f = self.find.get_or_insert_with(|| Find {
            query: TextEdit::new(""),
            scope: Side::Translation,
            results: Results::default(),
            current: 0,
            generation: 0,
            searching: false,
            searched: String::new(),
            search: Search::default(),
            due: None,
            book_generation: (0, 0),
            refresh: None,
        });
        f.query.focused = true;
        f.query.anchor = 0;
        f.query.focus = f.query.text.len();
        tx.focus("find-input", false);
    }

    pub fn search_find(&mut self) {
        let Some(f) = &mut self.find else {
            return;
        };
        f.restart();
        f.book_generation = self.lib.text_generation();
    }

    pub fn poll_find(&mut self, tx: &mut Tx) -> bool {
        let Some(f) = &mut self.find else {
            return false;
        };
        let changed = f.schedule(tx.clock(), self.lib.text_generation());
        let Some(due) = f.due else {
            return changed;
        };
        if tx.clock() < due {
            tx.wake_at(due);
            return changed;
        }
        f.due = None;
        let generation = f.generation;
        let job = self
            .lib
            .find_job(f.query.text.clone(), f.scope, f.search.clone());
        self.bg
            .spawn(move || Msg::Found(generation, job().map_err(|e| e.to_string())));
        true
    }

    pub fn found(&mut self, generation: u64, result: Result<Results, String>, tx: &mut Tx) {
        self.poll_find(tx);
        let Some(f) = &mut self.find else {
            return;
        };
        if f.generation != generation {
            return;
        }
        f.searching = false;
        match result {
            Ok(results) => {
                let refresh = f.refresh.take();
                f.current = refresh.as_ref().map_or(0, |r| r.current(&results));
                f.results = results;
                if refresh.is_none() {
                    self.jump_find(tx);
                }
            }
            Err(e) => self.toast = Some(format!("Could not search: {e}")),
        }
    }

    pub fn jump_find(&mut self, tx: &mut Tx) {
        self.poll_find(tx);
        let Some(hit) = self
            .find
            .as_ref()
            .and_then(|f| f.results.hits.get(f.current))
        else {
            return;
        };
        let (chapter, block, side) = (hit.chapter, hit.block, hit.side);
        self.go_block_side(tx, chapter, block, side);
    }

    pub fn go_block_side(&mut self, tx: &mut Tx, chapter: ChapterId, block: BlockId, side: Side) {
        if let Some(at) = self.lib.chapter_index(chapter) {
            self.source = side == Side::Source;
            self.open_chapter(at);
            self.pending_scroll = None;
            tx.scroll_key_to(format!("row{}", block.0), 180.0);
            tx.scroll_key_to(format!("ch{at}"), 200.0);
        }
    }

    pub fn step_find(&mut self, tx: &mut Tx, backwards: bool) {
        self.poll_find(tx);
        if let Some(f) = &mut self.find {
            let n = f.results.hits.len();
            if n == 0 {
                return;
            }
            f.current = (f.current + if backwards { n - 1 } else { 1 }) % n;
        }
        self.jump_find(tx);
    }

    pub fn close_find(&mut self, tx: &mut Tx) {
        self.find = None;
        tx.focus("doc", false);
    }

    pub fn toggle_find_scope(&mut self, tx: &mut Tx) {
        let Some(f) = &mut self.find else {
            return;
        };
        f.scope = match f.scope {
            Side::Source => Side::Translation,
            Side::Translation => Side::Source,
        };
        self.search_find();
        self.poll_find(tx);
    }
}

pub fn key(s: &mut State, k: &KeyEvent, tx: &mut Tx) -> bool {
    match k.key {
        Key::Command('f') if k.mods.shift && s.find.as_ref().is_some_and(|f| f.query.focused) => {
            s.toggle_find_scope(tx)
        }
        Key::Command('f') => s.open_find(tx),
        Key::Escape => s.close_find(tx),
        Key::Enter => s.step_find(tx, k.mods.shift),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn find_debounce_observes_direct_text_changes() {
        let mut f = Find {
            query: TextEdit::new(""),
            scope: Side::Source,
            results: Results::default(),
            current: 0,
            generation: 0,
            searching: false,
            searched: String::new(),
            search: Search::default(),
            due: None,
            book_generation: (0, 0),
            refresh: None,
        };
        assert!(!f.schedule(1.0, (0, 0)));
        assert_eq!(f.due, None);
        f.query.text = "타".into();
        assert!(f.schedule(2.0, (0, 0)));
        assert_eq!(f.due, Some(2.15));
        assert!(f.searching);
        let superseded = f.search.clone();
        f.query.text = "탑".into();
        assert!(f.schedule(2.1, (0, 0)));
        assert_eq!(f.due, Some(2.1 + 0.15));
        assert!(superseded.cancelled());
        assert!(!f.schedule(2.2, (0, 0)));
        assert_eq!(f.due, Some(2.1 + 0.15));
        f.due = None;
        assert!(!f.schedule(3.0, (0, 0)));
        assert_eq!(f.due, None);
        for query in ["", "  "] {
            f.query.text = query.into();
            assert!(f.schedule(4.0, (0, 0)));
            assert!(!f.searching);
            assert!(f.results.hits.is_empty());
            assert_eq!(f.due, None);
        }
    }

    #[test]
    fn find_restarts_after_book_text_refresh_or_reload() {
        for reload in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("find.cipher");
            cipher_engine::import(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/the-quiet-tower"),
                &path,
            )
            .unwrap();
            let mut s = State::new(Library::open(&path).unwrap());
            let mut tx = Tx::default();
            s.open_find(&mut tx);
            s.find.as_mut().unwrap().query.text = "tower".into();
            s.poll_find(&mut tx);
            let f = s.find.as_mut().unwrap();
            f.due = None;
            let generation = f.generation;
            let search = f.search.clone();
            let results =
                s.lib
                    .find_job("tower".into(), Side::Translation, search.clone())()
                .unwrap();
            let block = results.hits[0].block;
            s.found(generation, Ok(results), &mut tx);
            assert!(!s.find.as_ref().unwrap().results.hits.is_empty());
            let stale =
                s.lib
                    .find_job("tower".into(), Side::Translation, search.clone())()
                .unwrap();
            s.lib
                .book_mut()
                .apply(cipher_book::Change::EditBlock {
                    block,
                    text: "A quiet place.".into(),
                    hand: true,
                })
                .unwrap();
            if reload {
                s.lib.reload().unwrap();
            } else {
                s.lib.refresh_chapters(&[0]).unwrap();
            }
            s.poll_find(&mut tx);
            let f = s.find.as_ref().unwrap();
            assert_ne!(f.generation, generation);
            assert!(f.searching);
            assert!(f.results.hits.is_empty());
            assert!(search.cancelled());
            s.found(generation, Ok(stale), &mut tx);
            assert!(s.find.as_ref().unwrap().results.hits.is_empty());
            let f = s.find.as_mut().unwrap();
            f.due = None;
            let generation = f.generation;
            let results = s.lib.find_job("tower".into(), f.scope, f.search.clone())().unwrap();
            assert!(results.hits.iter().all(|h| h.block != block));
            s.found(generation, Ok(results), &mut tx);
            assert!(!s.find.as_ref().unwrap().searching);
        }
    }

    #[test]
    fn find_refresh_keeps_location_and_selected_hit() {
        for screen in [crate::state::Screen::History, crate::state::Screen::Reading] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("find.cipher");
            cipher_engine::import(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/the-quiet-tower"),
                &path,
            )
            .unwrap();
            let mut s = State::new(Library::open(&path).unwrap());
            let mut tx = Tx::default();
            s.open_find(&mut tx);
            s.find.as_mut().unwrap().query.text = "tower".into();
            s.poll_find(&mut tx);
            let f = s.find.as_mut().unwrap();
            f.due = None;
            let generation = f.generation;
            let results = s.lib.find_job("tower".into(), f.scope, f.search.clone())().unwrap();
            let removed = results.hits[0].block;
            s.found(generation, Ok(results), &mut tx);
            let f = s.find.as_mut().unwrap();
            f.current = f.results.hits.len() - 1;
            let old_index = f.current;
            let selected = f.results.hits[f.current].clone();
            assert_ne!(removed, selected.block);
            s.lib.open_chapter(1).unwrap();
            s.screen = screen;
            s.source = true;
            s.lib
                .book_mut()
                .apply(cipher_book::Change::EditBlock {
                    block: removed,
                    text: "A quiet place.".into(),
                    hand: true,
                })
                .unwrap();
            for _ in 0..2 {
                s.lib.refresh_chapters(&[0]).unwrap();
                s.poll_find(&mut tx);
            }
            let f = s.find.as_mut().unwrap();
            f.due = None;
            let generation = f.generation;
            let results = s.lib.find_job("tower".into(), f.scope, f.search.clone())().unwrap();
            let mut tx = Tx::default();
            let before = format!("{tx:?}");
            s.found(generation, Ok(results), &mut tx);
            assert_eq!(format!("{tx:?}"), before);
            assert!(s.screen == screen);
            assert_eq!(s.lib.current(), 1);
            assert!(s.source);
            let f = s.find.as_ref().unwrap();
            assert!(f.current < old_index);
            let hit = &f.results.hits[f.current];
            assert_eq!(hit.chapter, selected.chapter);
            assert_eq!(hit.block, selected.block);
            assert_eq!(hit.range, selected.range);

            s.lib.refresh_chapters(&[0]).unwrap();
            s.poll_find(&mut tx);
            let f = s.find.as_mut().unwrap();
            f.due = None;
            let generation = f.generation;
            let mut tx = Tx::default();
            s.found(generation, Err("search failed".into()), &mut tx);
            assert!(s.screen == screen);
            assert_eq!(s.lib.current(), 1);
            assert_eq!(format!("{tx:?}"), before);
            assert_eq!(s.toast.as_deref(), Some("Could not search: search failed"));

            s.toggle_find_scope(&mut tx);
            assert!(s.find.as_ref().unwrap().refresh.is_none());
        }
    }

    #[test]
    fn find_refresh_selects_nearest_remaining_hit() {
        let hit = |block, start| Hit {
            chapter: ChapterId(1),
            block: BlockId(block),
            side: Side::Translation,
            range: start..start + 5,
        };
        let refresh = Refresh {
            index: 2,
            hit: Some(hit(2, 30)),
        };
        let results = Results {
            hits: vec![hit(1, 0), hit(2, 25), hit(2, 50)],
            more: false,
        };
        assert_eq!(refresh.current(&results), 1);
        let results = Results {
            hits: vec![hit(1, 0), hit(3, 0)],
            more: false,
        };
        assert_eq!(refresh.current(&results), 1);
        assert_eq!(refresh.current(&Results::default()), 0);
    }

    #[test]
    fn find_mixed_scripts_and_offsets() {
        let text = "포혈공(怖血公) met Ärzte ÄRZTE カナ ПрИвЕт";
        for (q, count) in [
            ("공(怖", 1),
            ("ärzte", 2),
            ("привет", 1),
            ("カナ", 1),
            ("missing", 0),
            ("  ", 0),
            ("", 0),
        ] {
            let ranges = matches(text, q);
            assert_eq!(ranges.len(), count, "{q}");
            for r in ranges {
                assert!(text.get(r).is_some());
            }
        }
        assert_eq!(matches("aaaaa", "AA"), vec![0..2, 2..4]);
        assert_eq!(matches("ςΣσ", "σ").len(), 3);
    }

    #[test]
    fn find_large_book_job_is_send_and_capped() {
        use cipher_book::{BookMeta, Change};
        let start = std::time::Instant::now();
        let dir = tempfile::tempdir().unwrap();
        let mut book = Book::create(
            dir.path().join("large.cipher"),
            BookMeta {
                title: "Find test".into(),
                source_lang: "ko".into(),
                target_lang: "en".into(),
            },
        )
        .unwrap();
        for number in 1..=1500 {
            let source = (0..40)
                .map(|i| {
                    if i == 0 {
                        "**포혈공(怖血公)** met Ärzte カナ Привет".into()
                    } else {
                        "포혈공 walks through the tower".into()
                    }
                })
                .collect();
            book.apply(Change::AddChapter {
                number: Some(number),
                title: None,
                source,
            })
            .unwrap();
        }
        drop(book);
        let lib = Library::open_quick(&dir.path().join("large.cipher")).unwrap();
        let job = lib.find_job("ärzte".into(), Side::Source, Search::default());
        let result = std::thread::spawn(job).join().unwrap().unwrap();
        assert_eq!(result.hits.len(), 1500);
        assert!(!result.more);
        for h in &result.hits {
            assert_eq!(h.range, 25..31);
        }
        let result =
            std::thread::spawn(lib.find_job("포혈공".into(), Side::Source, Search::default()))
                .join()
                .unwrap()
                .unwrap();
        assert_eq!(result.hits.len(), LIMIT);
        assert!(result.more);
        assert!(
            std::thread::spawn(lib.find_job("".into(), Side::Source, Search::default()))
                .join()
                .unwrap()
                .unwrap()
                .hits
                .is_empty()
        );
        let search = Search::default();
        let superseded = lib.find_job("tower".into(), Side::Source, search.clone());
        let _replacement = search.next();
        let job = lib.find_job("tower".into(), Side::Source, Search::default());
        drop(lib);
        std::fs::remove_file(dir.path().join("large.cipher")).unwrap();
        let cancelled = std::thread::spawn(superseded).join().unwrap().unwrap();
        assert!(
            cancelled.hits.is_empty(),
            "superseded job exits without opening the Book"
        );
        assert!(std::thread::spawn(job).join().unwrap().is_err());
        eprintln!(
            "find_large_book_job_is_send_and_capped: {:?}",
            start.elapsed()
        );
    }
}
