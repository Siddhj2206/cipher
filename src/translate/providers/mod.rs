//! Provider abstraction for LLM backends
//!
//! Each provider is in its own file for easy extension.

pub mod gemini;
pub mod openai;

use crate::config::{GlobalConfig, ProviderKind};
use crate::translate::{TranslationRequest, TranslationResponse};
use anyhow::Result;

/// Trait for LLM providers
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// Translate a chapter given the request
    async fn translate(&self, req: TranslationRequest) -> Result<TranslationResponse>;
}

/// Parameters for provider construction
pub struct ProviderParams {
    pub api_key: String,
    pub model: String,
}

/// Build a provider from global config and profile name
pub fn build_provider(config: &GlobalConfig, profile_name: &str) -> Result<Box<dyn Provider>> {
    let profile = config
        .resolve_profile(profile_name)
        .ok_or_else(|| anyhow::anyhow!("Profile '{}' not found", profile_name))?;

    let provider_config = config
        .resolve_provider(&profile.provider)
        .ok_or_else(|| anyhow::anyhow!("Provider '{}' not found", profile.provider))?;

    let api_key = config
        .get_provider_key_by_label(&profile.provider, profile.key.as_deref())
        .ok_or_else(|| {
            if let Some(label) = profile.key.as_deref() {
                anyhow::anyhow!(
                    "No API key labeled '{}' for provider '{}'",
                    label,
                    profile.provider
                )
            } else {
                anyhow::anyhow!("No API key for provider '{}'", profile.provider)
            }
        })?;

    let params = ProviderParams {
        api_key: api_key.to_string(),
        model: profile.model.clone(),
    };

    match provider_config.kind {
        ProviderKind::Gemini => Ok(Box::new(gemini::GeminiProvider::new(params)?)),
        ProviderKind::Openai => Ok(Box::new(openai::OpenAiProvider::new(params, None)?)),
        ProviderKind::OpenaiCompatible => {
            let base_url = provider_config
                .base_url
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("OpenAI-compatible provider requires base_url"))?;
            Ok(Box::new(openai::OpenAiProvider::new(
                params,
                Some(base_url),
            )?))
        }
    }
}
