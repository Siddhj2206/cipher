//! Review: grouped for Renames, inline one by one for the rest, then the toast and Undo.

use cipher_book::{BlockId, ProposedEdit, ProposedEditId, Suggestion, SuggestionId, UndoOutcome};
use rune::Tx;

use super::{Finished, Review};
use crate::state::State;

/// The edits still to decide: the ones with new text first, then the Blocks the model chose to
/// leave as they are (accepting one of those only dismisses it).
pub fn queue(s: &Suggestion) -> Vec<&ProposedEdit> {
    let pending = |e: &&ProposedEdit| e.state == cipher_book::EditState::Pending;
    let mut q: Vec<&ProposedEdit> = s
        .edits
        .iter()
        .filter(pending)
        .filter(|e| e.after.is_some())
        .collect();
    q.extend(s.edits.iter().filter(pending).filter(|e| e.after.is_none()));
    q
}

/// The pending edit on `block` in the Suggestion under Review, and whether it is the current one.
pub fn edit_in_review(s: &State, block: BlockId) -> Option<(&ProposedEdit, bool)> {
    let review = s.fix.review.as_ref()?;
    let sug = s.lib.suggestion(review.suggestion)?;
    sug.edits
        .iter()
        .find(|e| e.block == block && e.state == cipher_book::EditState::Pending)
        .map(|e| (e, e.id == review.edit))
}

fn finished_for(sug: &Suggestion) -> Finished {
    Finished {
        suggestion: sug.id,
        chapter: sug.chapter,
        block: sug.block,
        quote: sug.quote.clone(),
        said: sug.said.clone(),
        scope: sug.scope,
        revisions: Vec::new(),
        chapters: Vec::new(),
        applied: 0,
        stale: 0,
        skipped: 0,
    }
}

impl State {
    /// Writes a batch of decisions as one Revision and adds what happened to `acc`.
    fn decide_into(&mut self, acc: &mut Finished, decisions: Vec<(ProposedEditId, bool)>) -> bool {
        let skipped = decisions.iter().filter(|d| !d.1).count();
        match self.lib.decide(decisions) {
            Ok(d) => {
                acc.revisions.push(d.revision);
                acc.applied += d.applied;
                acc.stale += d.stale;
                acc.skipped += skipped;
                for c in d.chapters {
                    if !acc.chapters.contains(&c) {
                        acc.chapters.push(c);
                    }
                }
                true
            }
            Err(e) => {
                self.toast = Some(format!("Could not apply: {e}"));
                false
            }
        }
    }

    /// "Accept all N" on a note: every pending edit in one Revision. Edits the model skipped
    /// are dismissed with it.
    pub fn accept_all(&mut self, id: SuggestionId) {
        let Some(sug) = self.lib.suggestion(id) else {
            return;
        };
        let mut acc = finished_for(sug);
        let decisions = queue(sug)
            .into_iter()
            .map(|e| (e.id, e.after.is_some()))
            .collect();
        if self.decide_into(&mut acc, decisions) {
            self.record(acc);
        }
    }

    /// "Dismiss" on a note whose edits all went stale: nothing is written.
    pub fn dismiss_stale(&mut self, id: SuggestionId) {
        let Some(sug) = self.lib.suggestion(id) else {
            return;
        };
        let decisions: Vec<(ProposedEditId, bool)> = sug
            .edits
            .iter()
            .filter(|e| e.state == cipher_book::EditState::Stale)
            .map(|e| (e.id, false))
            .collect();
        let mut acc = finished_for(sug);
        self.decide_into(&mut acc, decisions);
    }

    /// Keeps a finished Review for Undo and says what it did.
    fn record(&mut self, acc: Finished) {
        self.toast = Some(acc.summary());
        if acc.applied > 0 {
            self.toast_undo = Some(acc.suggestion);
            self.fix.finished.retain(|f| f.suggestion != acc.suggestion);
            self.fix.finished.push(acc);
        } else {
            self.toast_undo = None;
        }
    }

    /// Undoes what a Review wrote. Blocks edited since stay as they are and are reported.
    pub fn undo(&mut self, id: SuggestionId) {
        let Some(at) = self.fix.finished.iter().position(|f| f.suggestion == id) else {
            return;
        };
        let fin = self.fix.finished.remove(at);
        let mut conflicts = 0;
        for &rev in fin.revisions.iter().rev() {
            match self.lib.undo_review(rev, &fin.chapters) {
                Ok(UndoOutcome::Undone { skipped, .. }) => conflicts += skipped.len(),
                Ok(UndoOutcome::AlreadyUndone { .. }) => {}
                Ok(UndoOutcome::Conflict { conflicts: c }) => conflicts += c.len(),
                Err(e) => {
                    self.toast = Some(format!("Could not undo: {e}"));
                    return;
                }
            }
        }
        self.toast_undo = None;
        self.toast = Some(if conflicts == 0 {
            "Undone".to_string()
        } else {
            format!(
                "Undone, except {} changed since",
                crate::view::plural(conflicts, "block")
            )
        });
    }

    // ---- inline Review ----

    /// Opens a Review on the Suggestion's first pending edit, without moving the page.
    pub fn begin_review(&mut self, id: SuggestionId) -> Option<ProposedEditId> {
        let sug = self.lib.suggestion(id)?;
        let first = queue(sug).first().map(|e| e.id)?;
        self.fix.review = Some(Review {
            suggestion: id,
            edit: first,
            back: (self.lib.current(), self.scroll_y.get()),
            acc: finished_for(sug),
        });
        Some(first)
    }

    pub fn start_review(&mut self, tx: &mut Tx, id: SuggestionId) {
        if let Some(first) = self.begin_review(id) {
            self.show_edit(tx, first);
        }
    }

    /// Shows a Proposed Edit in its Chapter, which may not be the one that is open.
    pub fn show_edit(&mut self, tx: &mut Tx, edit: ProposedEditId) {
        let Some(review) = &self.fix.review else {
            return;
        };
        let Some(e) = self
            .lib
            .suggestion(review.suggestion)
            .and_then(|s| s.edits.iter().find(|e| e.id == edit))
        else {
            return;
        };
        let (block, chapter) = (e.block, e.chapter);
        if let Some(r) = &mut self.fix.review {
            r.edit = edit;
        }
        self.source = false;
        if let Some(at) = self.lib.chapter_index(chapter) {
            if at != self.lib.current() {
                self.open_chapter(at);
                self.pending_scroll = None;
            }
            tx.scroll_key_to(format!("row{}", block.0), 160.0);
            tx.scroll_key_to(format!("ch{at}"), 200.0);
        }
    }

    /// Moves to the next (or previous) pending edit, wrapping.
    pub fn step_edit(&mut self, tx: &mut Tx, dir: i32) {
        if let Some(next) = self.neighbour(dir) {
            self.show_edit(tx, next);
        }
    }

    fn neighbour(&self, dir: i32) -> Option<ProposedEditId> {
        let review = self.fix.review.as_ref()?;
        let q = queue(self.lib.suggestion(review.suggestion)?);
        let at = q.iter().position(|e| e.id == review.edit)?;
        let n = q.len() as i32;
        (n > 1).then(|| q[(at as i32 + dir).rem_euclid(n) as usize].id)
    }

    /// Accepts or skips the edit on screen and moves on; at the end, leaves with a summary.
    pub fn decide_current(&mut self, tx: &mut Tx, accept: bool) {
        let Some(review) = self.fix.review.clone() else {
            return;
        };
        let successor = self.neighbour(1);
        let mut acc = review.acc.clone();
        if !self.decide_into(&mut acc, vec![(review.edit, accept)]) {
            return;
        }
        if let Some(r) = &mut self.fix.review {
            r.acc = acc;
        }
        match successor {
            Some(next) if self.edit_pending(next) => self.show_edit(tx, next),
            _ => self.leave_review(tx),
        }
    }

    /// Shift+A: accepts every edit still pending, in one Revision.
    pub fn accept_rest(&mut self, tx: &mut Tx) {
        let Some(review) = self.fix.review.clone() else {
            return;
        };
        let Some(sug) = self.lib.suggestion(review.suggestion) else {
            return;
        };
        let decisions = queue(sug)
            .into_iter()
            .map(|e| (e.id, e.after.is_some()))
            .collect();
        let mut acc = review.acc.clone();
        if self.decide_into(&mut acc, decisions) {
            if let Some(r) = &mut self.fix.review {
                r.acc = acc;
            }
            self.leave_review(tx);
        }
    }

    fn edit_pending(&self, edit: ProposedEditId) -> bool {
        self.fix.review.as_ref().is_some_and(|r| {
            self.lib.suggestion(r.suggestion).is_some_and(|s| {
                s.edits
                    .iter()
                    .any(|e| e.id == edit && e.state == cipher_book::EditState::Pending)
            })
        })
    }

    /// Esc, or the last edit decided: back to where the user was reading.
    pub fn leave_review(&mut self, tx: &mut Tx) {
        let Some(review) = self.fix.review.take() else {
            return;
        };
        let (index, y) = review.back;
        self.open_chapter(index);
        self.pending_scroll = None;
        tx.scroll_to_y("doc", y);
        tx.scroll_key_to(format!("ch{index}"), 200.0);
        let acc = review.acc;
        if !acc.revisions.is_empty() {
            self.record(acc);
        }
    }
}
