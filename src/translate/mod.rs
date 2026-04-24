pub mod cmd;
pub mod prompt;
pub mod providers;
pub mod types;

pub use crate::translate::cmd::{TranslateOptions, translate_book};
pub use crate::translate::types::{
    GlossaryExtractionRequest, ProviderGlossaryResult, ProviderTextResult,
    ProviderTranslationResult, RepairRequest, TranslationRequest, TranslationResponse,
    TranslationUsage,
};

use crate::config::GlobalConfig;
use crate::glossary::GlossaryTerm;
use anyhow::{Context, Result};

pub struct Translator {
    provider: Box<dyn providers::Provider>,
}

impl Translator {
    pub fn from_config(config: &GlobalConfig, profile_name: &str) -> Result<Self> {
        let provider = providers::build_provider(config, profile_name)
            .with_context(|| format!("Failed to build provider for profile '{}'", profile_name))?;

        Ok(Self { provider })
    }

    pub async fn translate_chapter(
        &self,
        chapter_text: &str,
        glossary_terms: &[GlossaryTerm],
        style_guide: Option<String>,
    ) -> Result<ProviderTextResult> {
        let request = TranslationRequest::new(chapter_text.to_string())
            .with_glossary_terms(glossary_terms.to_vec())
            .with_style_guide(style_guide);

        self.provider.translate(request).await
    }

    pub async fn repair_chapter(
        &self,
        chapter_text: &str,
        failed_translation: String,
        glossary_terms: &[GlossaryTerm],
        style_guide: Option<String>,
        validation_errors: Vec<String>,
    ) -> Result<ProviderTextResult> {
        let request = RepairRequest::new(chapter_text.to_string(), failed_translation)
            .with_glossary_terms(glossary_terms.to_vec())
            .with_style_guide(style_guide)
            .with_validation_errors(validation_errors);

        self.provider.repair(request).await
    }

    pub async fn extract_glossary(
        &self,
        chapter_text: &str,
        translated_markdown: String,
    ) -> Result<ProviderGlossaryResult> {
        let request = GlossaryExtractionRequest::new(chapter_text.to_string(), translated_markdown);
        self.provider.extract_glossary(request).await
    }
}
