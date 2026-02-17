pub mod cmd;
pub mod prompt;
pub mod providers;
pub mod types;

pub use crate::translate::cmd::{TranslateOptions, translate_book};
pub use crate::translate::types::{TranslationRequest, TranslationResponse};

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
    ) -> Result<TranslationResponse> {
        let request = TranslationRequest::new(chapter_text.to_string())
            .with_glossary_terms(glossary_terms.to_vec());

        self.provider.translate(request).await
    }
}
