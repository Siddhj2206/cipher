//! Provider abstraction for LLM backends
//!
//! Each provider is in its own file for easy extension.

pub mod gemini;
pub mod openai;

use crate::config::{GlobalConfig, ProviderKind};
use crate::translate::{
    GlossaryExtractionRequest, ProviderGlossaryResult, ProviderTextResult, RepairRequest,
    TranslationRequest,
};
use anyhow::Result;

/// Trait for LLM providers
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// Translate a chapter given the request
    async fn translate(&self, req: TranslationRequest) -> Result<ProviderTextResult>;

    /// Repair a previously failed translation
    async fn repair(&self, req: RepairRequest) -> Result<ProviderTextResult>;

    /// Extract glossary terms from accepted translation output
    async fn extract_glossary(
        &self,
        req: GlossaryExtractionRequest,
    ) -> Result<ProviderGlossaryResult>;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiKey, GlobalConfig, ProfileConfig, ProviderConfig, ProviderKind};
    use std::collections::BTreeMap;

    fn config_with_profile() -> GlobalConfig {
        let mut config = GlobalConfig {
            default_profile: None,
            providers: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        config.providers.insert(
            "gemini".to_string(),
            ProviderConfig {
                kind: ProviderKind::Gemini,
                keys: vec![ApiKey {
                    value: "test-key".to_string(),
                    name: Some("default".to_string()),
                }],
                base_url: None,
            },
        );
        config.profiles.insert(
            "valid".to_string(),
            ProfileConfig {
                provider: "gemini".to_string(),
                model: "gemini-2.5-flash".to_string(),
                key: Some("default".to_string()),
            },
        );
        config
    }

    fn assert_provider_error(config: &GlobalConfig, name: &str, expected: &str) {
        match build_provider(config, name) {
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains(expected), "expected '{expected}' in '{msg}'");
            }
            Ok(_) => panic!("expected error for profile '{name}'"),
        }
    }

    #[test]
    fn build_provider_profile_not_found() {
        assert_provider_error(&GlobalConfig::default(), "nonexistent", "not found");
    }

    #[test]
    fn build_provider_provider_not_found() {
        let mut config = config_with_profile();
        config.profiles.insert(
            "bad".to_string(),
            ProfileConfig {
                provider: "nonexistent".to_string(),
                model: "m".to_string(),
                key: None,
            },
        );
        assert_provider_error(&config, "bad", "not found");
    }

    #[test]
    fn build_provider_missing_api_key_label() {
        let mut config = config_with_profile();
        config.profiles.insert(
            "nokey".to_string(),
            ProfileConfig {
                provider: "gemini".to_string(),
                model: "m".to_string(),
                key: Some("nonexistent-label".to_string()),
            },
        );
        assert_provider_error(&config, "nokey", "No API key");
    }

    #[test]
    fn build_provider_no_api_key_at_all() {
        let mut config = config_with_profile();
        config.providers.insert(
            "empty-provider".to_string(),
            ProviderConfig {
                kind: ProviderKind::Gemini,
                keys: vec![],
                base_url: None,
            },
        );
        config.profiles.insert(
            "empty-key".to_string(),
            ProfileConfig {
                provider: "empty-provider".to_string(),
                model: "m".to_string(),
                key: None,
            },
        );
        assert_provider_error(&config, "empty-key", "No API key");
    }
}
