//! Plain owned values returned by reads and accepted by [`crate::Change`].
//! Names follow `CONTEXT.md`.

use serde::{Deserialize, Serialize};

macro_rules! ids {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub i64);
    )*};
}

ids!(
    ChapterId,
    BlockId,
    PairId,
    TermId,
    StyleRuleId,
    AddressId,
    StoryNoteId,
    SuggestionId,
    ProposedEditId,
    RevisionId,
    JobId,
);

/// Unix seconds.
pub type Timestamp = i64;

/// Declares an enum stored in SQLite as a fixed lowercase string.
macro_rules! text_enum {
    ($(#[$doc:meta])* $name:ident { $($variant:ident = $text:literal),* $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name { $($variant),* }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),* }
            }

            pub(crate) fn parse(s: &str) -> crate::Result<Self> {
                match s {
                    $($text => Ok(Self::$variant),)*
                    other => Err(crate::Error::Corrupt(format!(
                        "unknown {} {other:?}", stringify!($name)
                    ))),
                }
            }
        }
    };
}

text_enum!(
    /// Which text a Block belongs to.
    Side { Source = "source", Translation = "translation" }
);
text_enum!(TermStatus {
    Accepted = "accepted",
    Pending = "pending",
    Dismissed = "dismissed",
});
text_enum!(
    /// How far a Suggestion reaches. See `CONTEXT.md` › Scope.
    Scope { Here = "here", Everywhere = "everywhere", FromNowOn = "from_now_on" }
);
text_enum!(
    /// `Working` while Jobs are producing Proposed Edits, `Ready` for Review,
    /// `Done` once every Proposed Edit is decided.
    SuggestionState { Working = "working", Ready = "ready", Done = "done" }
);
text_enum!(
    /// Who asked: the reader, in words, or the Cast page fixing a Term's Drift. A Drift
    /// Suggestion's `said` is for the reader; the model gets its own request.
    SuggestionKind { Reader = "reader", Drift = "drift" }
);
text_enum!(
    /// `Rename` edits are exact local replacements, reviewed as a group.
    EditOrigin { Rename = "rename", Model = "model" }
);
text_enum!(EditState {
    Pending = "pending",
    Accepted = "accepted",
    Rejected = "rejected",
    Stale = "stale",
});
text_enum!(RevisionKind {
    Import = "import",
    Chapter = "chapter",
    Translation = "translation",
    Review = "review",
    HandEdit = "hand_edit",
    Glossary = "glossary",
    Suggestion = "suggestion",
    Notes = "notes",
    Undo = "undo",
});
text_enum!(JobKind {
    Translate = "translate",
    Repair = "repair",
    SuggestBatch = "suggest_batch",
    ResolveSource = "resolve_source",
    CompactNotes = "compact_notes",
});
text_enum!(
    /// `queued → running → checking → done`, or `failed` with a reason,
    /// or `paused` until `resume_after` when the Endpoint reports a usage limit.
    JobState {
        Queued = "queued",
        Running = "running",
        Checking = "checking",
        Done = "done",
        Failed = "failed",
        Paused = "paused",
    }
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookMeta {
    pub title: String,
    pub source_lang: String,
    pub target_lang: String,
}

/// A Book's own pick of model: an Endpoint of the app's, by id, and one of its models.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice {
    pub endpoint: String,
    pub model: String,
}

/// Derived from the Chapter's Blocks and its latest translate Job; never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterState {
    /// No translation and no Job.
    Waiting,
    /// A translate Job is queued or paused.
    Queued,
    /// A translate Job is running or checking.
    Translating,
    /// Has translated Blocks.
    Translated,
    /// The latest translate Job failed and there is no translation.
    Failed,
    /// No source text and no translation: a Chapter missing from what was imported or pasted.
    /// Nothing to translate.
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChapterSummary {
    pub id: ChapterId,
    pub number: Option<i64>,
    pub title: Option<String>,
    pub state: ChapterState,
    /// Number of source Blocks.
    pub blocks: usize,
    /// The first source Block, for the Contents list.
    pub first_line: String,
    /// Translated Blocks marked Outdated.
    pub outdated: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub id: BlockId,
    pub side: Side,
    pub pair: Option<PairId>,
    pub text: String,
    pub hand_edited: bool,
    /// Translation side only: its source changed after it was written.
    pub outdated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub id: PairId,
    /// Why Repair could not fix this Pair, if it couldn't.
    pub flagged: Option<String>,
}

/// One Chapter with both sides. Blocks are in reading order.
#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub id: ChapterId,
    pub number: Option<i64>,
    pub title: Option<String>,
    pub source: Vec<Block>,
    pub translation: Vec<Block>,
    /// In reading order.
    pub pairs: Vec<Pair>,
}

impl Chapter {
    /// The Blocks on `side` that belong to `pair`, in order.
    pub fn blocks_in(&self, pair: PairId, side: Side) -> impl Iterator<Item = &Block> {
        let blocks = match side {
            Side::Source => &self.source,
            Side::Translation => &self.translation,
        };
        blocks.iter().filter(move |b| b.pair == Some(pair))
    }

    pub fn block(&self, id: BlockId) -> Option<&Block> {
        self.source
            .iter()
            .chain(&self.translation)
            .find(|b| b.id == id)
    }
}

/// The editable fields of a Term.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TermFields {
    pub source: String,
    pub rendering: String,
    pub kind: Option<String>,
    pub gender: Option<String>,
    pub note: Option<String>,
    /// The Translation may refer to this Term without naming it (by a pronoun, or not at all),
    /// so a Chapter that never renders it is not Drift. Forbidden Renderings are still checked.
    pub may_omit: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Term {
    pub id: TermId,
    pub fields: TermFields,
    pub status: TermStatus,
    pub first_chapter: Option<ChapterId>,
    /// Renderings that must not appear, sorted.
    pub forbidden: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyleRule {
    pub id: StyleRuleId,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AddressFields {
    pub speaker: String,
    pub listener: String,
    pub source_form: String,
    pub rendering: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormOfAddress {
    pub id: AddressId,
    pub fields: AddressFields,
}

/// Terms (all statuses), Style Rules and Forms of Address.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Glossary {
    pub terms: Vec<Term>,
    pub style_rules: Vec<StyleRule>,
    pub forms_of_address: Vec<FormOfAddress>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryNote {
    pub id: StoryNoteId,
    /// `None` for a compacted summary.
    pub chapter: Option<ChapterId>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProposedEdit {
    pub id: ProposedEditId,
    pub block: BlockId,
    pub chapter: ChapterId,
    /// The Block's text when the edit was proposed.
    pub before: String,
    /// `None` when the model skipped this Block.
    pub after: Option<String>,
    pub origin: EditOrigin,
    pub skip_reason: Option<String>,
    /// A pending edit whose Block no longer reads `before` is reported as `Stale`.
    pub state: EditState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub id: SuggestionId,
    pub chapter: ChapterId,
    pub block: BlockId,
    pub quote: String,
    pub said: String,
    pub scope: Scope,
    pub is_rename: bool,
    pub source_expr: Option<String>,
    pub state: SuggestionState,
    pub kind: SuggestionKind,
    pub created_at: Timestamp,
    pub edits: Vec<ProposedEdit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SuggestionFilter {
    pub chapter: Option<ChapterId>,
    /// Leave out Suggestions that are `Done`.
    pub open_only: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewProposedEdit {
    pub block: BlockId,
    pub before: String,
    pub after: Option<String>,
    pub origin: EditOrigin,
    pub skip_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewSuggestion {
    pub chapter: ChapterId,
    pub block: BlockId,
    pub quote: String,
    pub said: String,
    pub scope: Scope,
    pub is_rename: bool,
    pub source_expr: Option<String>,
    pub state: SuggestionState,
    pub kind: SuggestionKind,
    pub edits: Vec<NewProposedEdit>,
}

/// One Pair of a new Translation: contiguous source Blocks and the translated text.
#[derive(Debug, Clone, PartialEq)]
pub struct NewPair {
    /// Source Blocks of this Chapter, in order. More than one means a merge.
    pub source: Vec<BlockId>,
    /// Translated Blocks; at least one. Usually exactly one: a split is a line break inside it.
    pub translation: Vec<String>,
    pub flagged: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionSummary {
    pub id: RevisionId,
    pub kind: RevisionKind,
    pub label: String,
    pub created_at: Timestamp,
    pub undone_by: Option<RevisionId>,
}

/// A dry run of Undo, read from the Book's persistent history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoPreview {
    pub revision: RevisionSummary,
    pub chapters: Vec<ChapterId>,
    pub changes: Vec<(&'static str, usize)>,
    pub conflicts: Vec<Conflict>,
    pub conflict_chapters: Vec<(Conflict, ChapterId)>,
    pub already_undone_by: Option<RevisionId>,
}

/// What [`crate::Book::apply`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub revision: RevisionId,
    /// Ids of the rows created by each creating Change, in order:
    /// AddChapter → Chapter, AddSuggestion → Suggestion, UpsertTerm without id → Term,
    /// AddStyleRule, AddFormOfAddress and AddStoryNote → their row.
    pub created: Vec<i64>,
}

/// How [`crate::Book::undo`] treats rows that later work changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoMode {
    /// All or nothing. Use for structural Revisions (a Translation, an added Chapter),
    /// where a partial revert would leave a half-paired Chapter.
    Whole,
    /// Revert every row that is unchanged since and skip the rest. Use for Reviews and
    /// other Revisions made of independent Block edits: the skipped Blocks are shown as
    /// conflicts.
    SkipConflicts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoOutcome {
    /// `skipped` is empty unless the mode was `SkipConflicts`.
    Undone {
        revision: RevisionId,
        skipped: Vec<Conflict>,
    },
    AlreadyUndone {
        by: RevisionId,
    },
    /// `UndoMode::Whole` only: nothing was changed, because later work touched these rows.
    Conflict {
        conflicts: Vec<Conflict>,
    },
}

/// A row that changed after the Revision being undone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub entity: &'static str,
    pub id: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobSpec {
    pub kind: JobKind,
    pub parent: Option<JobId>,
    pub chapter: Option<ChapterId>,
    pub suggestion: Option<SuggestionId>,
    /// Opaque to the Book; the engine owns its format (JSON by convention).
    pub input: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: JobId,
    pub kind: JobKind,
    pub parent: Option<JobId>,
    pub chapter: Option<ChapterId>,
    pub suggestion: Option<SuggestionId>,
    pub state: JobState,
    pub input: Option<String>,
    /// Streamed output so far, for resuming.
    pub partial: String,
    pub attempts: i64,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    /// The Endpoint and model that did it, as "ChatGPT · gpt-6-luna".
    pub model: Option<String>,
    pub error: Option<String>,
    pub resume_after: Option<Timestamp>,
    pub updated_at: Timestamp,
}
