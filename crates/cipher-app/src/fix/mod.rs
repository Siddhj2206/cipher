//! Fixing while reading (`docs/design/desktop-ux.md` › Fixing while reading): the selection, the
//! fix box, the notes that Suggestions become, and the Review that follows.
//!
//! - `work`: the background worker, its Events, and Reach on a thread.
//! - `compose`: the selection chip and the fix box.
//! - `review`: grouped and inline Review, the toast and Undo.

mod compose;
mod review;
pub mod work;

use cipher_book::{BlockId, ChapterId, ProposedEditId, RevisionId, Scope, SuggestionId};
use cipher_engine::Reach;
use rune::TextEdit;

pub use review::{edit_in_review, queue};

/// The Scope chips, in the order Tab cycles them. The last is "Everywhere + glossary" for a
/// Term and "From now on" otherwise.
pub const SCOPES: [Scope; 3] = [Scope::Here, Scope::Everywhere, Scope::FromNowOn];

/// The text the user selected in a translated Block.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    pub block: BlockId,
    pub text: String,
    /// What it would reach Everywhere; `None` while that is being worked out.
    pub reach: Option<Reach>,
}

/// An inline Review in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    pub suggestion: SuggestionId,
    /// The edit on screen.
    pub edit: ProposedEditId,
    /// Where the user was reading: Chapter index and scroll.
    pub back: (usize, f32),
    /// What the Review has written so far.
    pub acc: Finished,
}

/// A Suggestion whose Review was applied in this session, kept so its note can offer Undo.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub suggestion: SuggestionId,
    pub chapter: ChapterId,
    pub block: BlockId,
    pub quote: String,
    pub said: String,
    pub scope: Scope,
    pub revisions: Vec<RevisionId>,
    pub chapters: Vec<ChapterId>,
    pub applied: usize,
    pub stale: usize,
    pub skipped: usize,
}

impl Finished {
    /// "142 applied, 1 changed since"
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.applied > 0 || (self.stale == 0 && self.skipped == 0) {
            parts.push(format!("{} applied", self.applied));
        }
        if self.stale > 0 {
            parts.push(format!("{} changed since", self.stale));
        }
        if self.skipped > 0 {
            parts.push(format!("{} skipped", self.skipped));
        }
        parts.join(", ")
    }
}

pub struct Fix {
    pub picked: Option<Picked>,
    pub composing: bool,
    pub composer: TextEdit,
    /// Index into [`SCOPES`].
    pub scope: usize,
    /// The user has chosen a Scope, so a Term found later does not move it.
    pub scope_chosen: bool,
    pub rename: bool,
    pub review: Option<Review>,
    pub finished: Vec<Finished>,
    /// Reach is being worked out on a thread.
    pub reach_busy: bool,
}

impl Fix {
    pub fn new() -> Fix {
        Fix {
            picked: None,
            composing: false,
            composer: TextEdit::new(""),
            scope: 0,
            scope_chosen: false,
            rename: false,
            review: None,
            finished: Vec::new(),
            reach_busy: false,
        }
    }

    /// The Scope the fix box would send.
    pub fn scope(&self) -> Scope {
        SCOPES[self.scope]
    }

    /// A Term's Rename lives in the glossary too.
    pub fn is_term(&self) -> bool {
        self.picked
            .as_ref()
            .and_then(|p| p.reach.as_ref())
            .is_some_and(|r| r.term.is_some())
    }
}

impl Default for Fix {
    fn default() -> Self {
        Fix::new()
    }
}
