pub mod cmd;
pub mod prompt;
pub mod providers;
pub mod types;

pub use crate::translate::cmd::{TranslateOptions, translate_book};
pub use crate::translate::types::{
    AcceptedTranslation, GlossaryExtractionRequest, ProviderGlossaryResult, ProviderTextResult,
    ProviderTranslationResult, RepairRequest, TranslationRequest, TranslationUsage,
};

use crate::book::OutputConfig;
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
        output_config: OutputConfig,
    ) -> Result<ProviderTextResult> {
        let request = TranslationRequest {
            chapter_markdown: chapter_text.to_string(),
            glossary_terms: glossary_terms.to_vec(),
            style_guide,
            output_config,
        };

        self.provider.translate(request).await
    }

    pub async fn repair_chapter(
        &self,
        chapter_text: &str,
        failed_translation: String,
        glossary_terms: &[GlossaryTerm],
        style_guide: Option<String>,
        validation_errors: Vec<String>,
        output_config: OutputConfig,
    ) -> Result<ProviderTextResult> {
        let request = RepairRequest {
            chapter_markdown: chapter_text.to_string(),
            glossary_terms: glossary_terms.to_vec(),
            style_guide,
            failed_translation,
            validation_errors,
            output_config,
        };

        self.provider.repair(request).await
    }

    pub async fn extract_glossary(
        &self,
        chapter_text: &str,
        translated_markdown: String,
        existing_glossary_terms: &[GlossaryTerm],
    ) -> Result<ProviderGlossaryResult> {
        let request = GlossaryExtractionRequest {
            chapter_markdown: chapter_text.to_string(),
            translated_markdown,
            existing_glossary_terms: existing_glossary_terms.to_vec(),
        };
        self.provider.extract_glossary(request).await
    }
}
