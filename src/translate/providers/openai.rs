//! OpenAI provider implementation
//!
//! Supports both OpenAI and OpenAI-compatible endpoints
//! - OpenAI: Uses Responses API (best structured output support)
//! - OpenAI-compatible: Uses Chat Completions API (more widely supported)

use crate::translate::prompt::build_prompt;
use crate::translate::providers::{Provider, ProviderParams};
use crate::translate::{TranslationRequest, TranslationResponse};
use anyhow::{Context, Result};
use rig::providers::openai;

pub struct OpenAiProvider {
    client: openai::Client,
    model: String,
    temperature: Option<f32>,
    max_tokens: Option<u64>,
    use_completions_api: bool,
}

impl OpenAiProvider {
    pub fn new(params: ProviderParams, base_url: Option<&str>) -> Result<Self> {
        let client = if let Some(url) = base_url {
            openai::Client::builder()
                .api_key(&params.api_key)
                .base_url(url)
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to build OpenAI client: {}", e))?
        } else {
            openai::Client::new(&params.api_key)
                .map_err(|e| anyhow::anyhow!("Failed to build OpenAI client: {}", e))?
        };

        // Use completions API for OpenAI-compatible endpoints (they typically don't support Responses API)
        let use_completions_api = base_url.is_some();

        Ok(Self {
            client,
            model: params.model,
            temperature: params.temperature,
            max_tokens: params.max_tokens,
            use_completions_api,
        })
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiProvider {
    async fn translate(&self, req: TranslationRequest) -> Result<TranslationResponse> {
        let prompt = build_prompt(&req);

        let response = if self.use_completions_api {
            // Use Chat Completions API (for OpenAI-compatible endpoints)
            let completions_client = self.client.clone().completions_api();
            let extractor = completions_client
                .extractor::<TranslationResponse>(&self.model)
                .preamble("You are a professional translator. Always return valid JSON matching the TranslationResponse schema.")
                .build();

            extractor
                .extract(&prompt)
                .await
                .context("Failed to extract translation from LLM (Chat Completions API)")?
        } else {
            // Use Responses API (for real OpenAI - best structured output support)
            let extractor = self
                .client
                .extractor::<TranslationResponse>(&self.model)
                .preamble("You are a professional translator. Always return valid JSON matching the TranslationResponse schema.")
                .build();

            extractor
                .extract(&prompt)
                .await
                .context("Failed to extract translation from LLM (Responses API)")?
        };

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_creation() {
        // This would need a real API key to test properly
        // For now, just verify the types compile
    }
}
