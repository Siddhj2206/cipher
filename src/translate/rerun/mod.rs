pub mod glossary;
pub(crate) mod types;

mod baseline;
mod decisions;

pub(crate) use baseline::{finalize_glossary_baseline, migrate_legacy_full_tracking};
pub(crate) use decisions::{build_glossary_rerun_plan, build_source_rerun_plan, combine_rerun_decisions};
pub(crate) use glossary::{build_chapter_glossary_usage, build_glossary_state};
pub(crate) use types::{GlossaryRerunPlan, RerunDecision, SourceRerunPlan};

