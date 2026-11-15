//! Pure text logic for Cipher: everything that can be done to text without a
//! model or a database. See `docs/design/architecture.md` › cipher-text and
//! `docs/design/translation-pipeline.md`.
//!
//! The crate is a set of modules, each a group of free functions over `&str`
//! and plain owned structs. There is no I/O, no clock and no randomness, so
//! every function is deterministic and testable with table tests.
//!
//! | Module | Job |
//! |---|---|
//! | [`align`] | pair an existing translation with its source, locally (imports) |
//! | [`blocks`] | split Chapter markdown into Blocks; render a Block's inline markdown |
//! | [`markers`] | write numbered `[n]` lines; parse a model response into Pairs plus trailer |
//! | [`matching`] | recall matcher, precision matcher (Hangul boundaries), English-side finder |
//! | [`checks`] | findings on Pairs: leftover script, Forbidden Renderings, missing names, length |
//! | [`rename`] | deterministic Rename with boundaries, case, possessives, articles, partial names |
//! | [`drift`] | per-Term, per-Chapter Drift |
//! | [`resolve`] | find the source expression behind a Selection from the Pairs that contain it |
//! | [`diff`] | word-level diff for Review |
//! | [`script`] | Hangul, kana and Han character classes shared by the other modules |
//!
//! Offsets are always **byte offsets into the `&str` the caller passed in or
//! received**, and always fall on `char` boundaries.

pub mod align;
pub mod blocks;
pub mod checks;
pub mod diff;
pub mod drift;
pub mod heading;
pub mod markers;
pub mod matching;
pub mod rename;
pub mod resolve;
pub mod script;
