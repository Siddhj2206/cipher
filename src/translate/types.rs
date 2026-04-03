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
    pub response: TranslationResponse,
    pub usage: TranslationUsage,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TranslationResponse {
    pub translation: String,
    pub new_glossary_terms: Vec<GlossaryTerm>,
}

#[derive(Debug, Clone)]
pub struct TranslationRequest {
    pub chapter_markdown: String,
    pub glossary_terms: Vec<GlossaryTerm>,
    pub style_guide: Option<String>,
    pub failed_translation: Option<String>,
    pub validation_errors: Vec<String>,
}

impl TranslationRequest {
    pub fn new(chapter_markdown: String) -> Self {
        Self {
            chapter_markdown,
            glossary_terms: Vec::new(),
            style_guide: None,
            failed_translation: None,
            validation_errors: Vec::new(),
        }
    }

    pub fn with_glossary_terms(mut self, terms: Vec<GlossaryTerm>) -> Self {
        self.glossary_terms = terms;
        self
    }

    pub fn with_style_guide(mut self, style_guide: Option<String>) -> Self {
        self.style_guide = style_guide;
        self
    }

    pub fn with_failed_translation(mut self, failed: String) -> Self {
        self.failed_translation = Some(failed);
        self
    }

    pub fn with_validation_errors(mut self, errors: Vec<String>) -> Self {
        self.validation_errors = errors;
        self
    }

    pub fn is_repair(&self) -> bool {
        self.failed_translation.is_some()
    }
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
