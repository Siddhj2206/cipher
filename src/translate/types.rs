use crate::glossary::GlossaryTerm;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TranslationResponse {
    pub translation: String,
    pub new_glossary_terms: Vec<GlossaryTerm>,
}

#[derive(Debug, Clone)]
pub struct TranslationRequest {
    pub chapter_markdown: String,
    pub glossary_terms: Vec<GlossaryTerm>,
    pub failed_translation: Option<String>,
    pub validation_errors: Vec<String>,
}

impl TranslationRequest {
    pub fn new(chapter_markdown: String) -> Self {
        Self {
            chapter_markdown,
            glossary_terms: Vec::new(),
            failed_translation: None,
            validation_errors: Vec::new(),
        }
    }

    pub fn with_glossary_terms(mut self, terms: Vec<GlossaryTerm>) -> Self {
        self.glossary_terms = terms;
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
