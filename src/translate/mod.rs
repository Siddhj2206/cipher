//! Translation module
//!
//! Provides LLM-backed translation with glossary integration.

pub mod providers;
pub mod prompt;
pub mod types;

pub use types::{TranslationRequest, TranslationResponse};
pub use providers::build_provider;

use crate::config::GlobalConfig;
use crate::glossary::GlossaryTerm;
use anyhow::{Context, Result};

/// High-level translator that handles chapter translation with glossary updates
pub struct Translator {
    provider: Box<dyn providers::Provider>,
}

impl Translator {
    /// Create a translator from global config and profile name
    pub fn from_config(config: &GlobalConfig, profile_name: &str) -> Result<Self> {
        let provider = build_provider(config, profile_name)
            .with_context(|| format!("Failed to create provider for profile '{}'", profile_name))?;
        
        Ok(Self { provider })
    }
    
    /// Translate a chapter and return the response (without auto-merging glossary)
    pub async fn translate_chapter(
        &self,
        chapter_text: &str,
        glossary_terms: &[GlossaryTerm],
    ) -> Result<TranslationResponse> {
        let request = TranslationRequest::new(chapter_text.to_string())
            .with_glossary_terms(glossary_terms.to_vec());
        
        self.provider.translate(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glossary::GlossaryTerm;

    #[test]
    fn test_translation_request() {
        let req = TranslationRequest::new("# Chapter 1\n\nHello".to_string());
        assert_eq!(req.chapter_markdown, "# Chapter 1\n\nHello");
        assert!(req.glossary_terms.is_empty());
    }

    #[test]
    fn test_translation_request_with_glossary() {
        let terms = vec![
            GlossaryTerm {
                term: "Magic".to_string(),
                og_term: Some("마법".to_string()),
                definition: "Supernatural power".to_string(),
                notes: None,
            },
        ];
        let req = TranslationRequest::new("Text".to_string()).with_glossary_terms(terms);
        assert_eq!(req.glossary_terms.len(), 1);
    }
}
