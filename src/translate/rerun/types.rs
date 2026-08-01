use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::glossary::{GlossaryTerm, InjectionMode};
use crate::state::ChapterState;

#[derive(Debug, Clone)]
pub(crate) struct RerunPlanContext<'a> {
    pub chapters: &'a [PathBuf],
    pub raw_dir: &'a Path,
    pub out_dir: &'a Path,
    pub chapter_states: &'a BTreeMap<String, ChapterState>,
    pub glossary: &'a [GlossaryTerm],
    pub injection_mode: InjectionMode,
}

#[derive(Debug, Clone)]
pub(crate) struct RerunDecision {
    pub reason: String,
    pub is_approximate: bool,
}

#[derive(Debug, Default)]
pub(crate) struct GlossaryRerunPlan {
    pub forced_chapters: BTreeMap<String, RerunDecision>,
    pub warnings: Vec<String>,
    pub changed_term_count: usize,
    pub approximate_smart_checks: usize,
}

#[derive(Debug, Default)]
pub(crate) struct SourceRerunPlan {
    pub forced_chapters: BTreeMap<String, RerunDecision>,
    pub untracked_chapters: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BaselineAction {
    KeepExisting,
    InitializeFromRunStart,
    CommitRunEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GlossaryBaselineOutcome {
    pub action: BaselineAction,
    pub remaining_forced_chapters: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LegacyTrackingMigration {
    pub migrated_chapters: usize,
    pub migrated_glossary_baseline: bool,
}

impl GlossaryRerunPlan {
    pub fn decision_for(&self, filename: &str) -> Option<&RerunDecision> {
        self.forced_chapters.get(filename)
    }
}

impl SourceRerunPlan {
    pub fn decision_for(&self, filename: &str) -> Option<&RerunDecision> {
        self.forced_chapters.get(filename)
    }
}

#[cfg(test)]
use crate::state::GlossaryStateTerm;

#[cfg(test)]
pub(crate) fn snapshot_fingerprints(
    terms: &BTreeMap<String, GlossaryStateTerm>,
) -> BTreeMap<String, String> {
    terms
        .iter()
        .map(|(key, term)| (key.clone(), term.fingerprint.clone()))
        .collect()
}
