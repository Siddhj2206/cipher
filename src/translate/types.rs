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
}

impl TranslationRequest {
    pub fn new(chapter_markdown: String) -> Self {
        Self {
            chapter_markdown,
            glossary_terms: Vec::new(),
        }
    }

    pub fn with_glossary_terms(mut self, terms: Vec<GlossaryTerm>) -> Self {
        self.glossary_terms = terms;
        self
    }
}
