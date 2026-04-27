use crate::book::{OutputConfig, StructuredChapter};
use crate::glossary::GlossaryTerm;
use serde::{Deserialize, Serialize};
use std::ops::AddAssign;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct TranslationUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct ProviderTranslationResult {
    pub response: AcceptedTranslation,
    pub usage: TranslationUsage,
}

#[derive(Debug, Clone)]
pub struct ProviderTextResult {
    pub chapter: StructuredChapter,
    pub usage: TranslationUsage,
}

#[derive(Debug, Clone)]
pub struct ProviderGlossaryResult {
    pub new_glossary_terms: Vec<GlossaryTerm>,
    pub usage: TranslationUsage,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AcceptedTranslation {
    pub chapter: StructuredChapter,
    pub new_glossary_terms: Vec<GlossaryTerm>,
}

#[derive(Debug, Clone)]
pub struct TranslationRequest {
    pub chapter_markdown: String,
    pub glossary_terms: Vec<GlossaryTerm>,
    pub style_guide: Option<String>,
    pub output_config: OutputConfig,
}

#[derive(Debug, Clone)]
pub struct RepairRequest {
    pub chapter_markdown: String,
    pub glossary_terms: Vec<GlossaryTerm>,
    pub style_guide: Option<String>,
    pub failed_translation: String,
    pub validation_errors: Vec<String>,
    pub output_config: OutputConfig,
}

#[derive(Debug, Clone)]
pub struct GlossaryExtractionRequest {
    pub chapter_markdown: String,
    pub translated_markdown: String,
    pub existing_glossary_terms: Vec<GlossaryTerm>,
}

impl From<rig::completion::Usage> for TranslationUsage {
    fn from(value: rig::completion::Usage) -> Self {
        Self {
            input_tokens: value.input_tokens,
            output_tokens: value.output_tokens,
            total_tokens: value.total_tokens,
            cached_input_tokens: value.cached_input_tokens,
            cache_creation_input_tokens: value.cache_creation_input_tokens,
        }
    }
}

impl AddAssign for TranslationUsage {
    fn add_assign(&mut self, other: Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
    }
}
