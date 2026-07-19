//! OpenAI provider implementation
//!
//! Supports both OpenAI and OpenAI-compatible endpoints
//! - OpenAI: Uses Responses API (best structured output support)
//! - OpenAI-compatible: Uses Chat Completions API (more widely supported)

use anyhow::Result;
use rig::providers::openai;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::book::StructuredChapter;
use crate::translate::prompt::{
    build_glossary_extraction_prompt, build_glossary_section, build_repair_prompt,
    build_style_section, build_translation_prompt,
};
use crate::translate::providers::shared::{self, HttpErrorMessages};
use crate::translate::providers::{Provider, ProviderParams};
use crate::translate::{
    GlossaryExtractionRequest, ProviderGlossaryResult, ProviderTextResult, RepairRequest,
    TranslationRequest,
};

const OPENAI_HTTP_MSGS: HttpErrorMessages = HttpErrorMessages {
    not_found: "Check your base URL and model name",
    unauthorized: "Check your API key",
    rate_limited: "Rate limit exceeded",
    server_error: "Provider issue",
};

pub struct OpenAiProvider {
    client: openai::Client,
    model: String,
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

        let use_completions_api = base_url.is_some();

        Ok(Self {
            client,
            model: params.model,
            use_completions_api,
        })
    }

    async fn extract_structured<T>(
        &self,
        prompt: String,
        preamble: &str,
    ) -> Result<(T, rig::completion::Usage)>
    where
        T: DeserializeOwned + Serialize + schemars::JsonSchema + Send + Sync + 'static,
    {
        let result = if self.use_completions_api {
            let completions_client = self.client.clone().completions_api();
            let extractor = completions_client
                .extractor::<T>(&self.model)
                .preamble(preamble)
                .retries(shared::EXTRACTOR_RETRIES)
                .build();

            extractor.extract_with_usage(&prompt).await
        } else {
            let extractor = self
                .client
                .extractor::<T>(&self.model)
                .preamble(preamble)
                .retries(shared::EXTRACTOR_RETRIES)
                .build();

            extractor.extract_with_usage(&prompt).await
        };

        match result {
            Ok(extracted) => Ok((extracted.data, extracted.usage)),
            Err(err) => {
                let detailed_error = shared::format_extraction_error(&err, &OPENAI_HTTP_MSGS);
                Err(anyhow::anyhow!("LLM request failed: {}", detailed_error))
            }
        }
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiProvider {
    async fn translate(&self, req: TranslationRequest) -> Result<ProviderTextResult> {
        let prompt = build_translation_prompt(&req);
        let (response, usage) = self
            .extract_structured::<shared::TranslationOnlyResponse>(
                prompt,
                shared::TRANSLATION_PREAMBLE,
            )
            .await?;

        Ok(ProviderTextResult {
            chapter: StructuredChapter {
                chapter_number: response.chapter_number,
                chapter_title: response.chapter_title,
                content: response.content,
            }
            .normalized(),
            usage: usage.into(),
        })
    }

    async fn repair(&self, req: RepairRequest) -> Result<ProviderTextResult> {
        let glossary_section = build_glossary_section(&req.glossary_terms);
        let style_section = build_style_section(&req.style_guide);
        let prompt = build_repair_prompt(&req, &glossary_section, &style_section);
        let (response, usage) = self
            .extract_structured::<shared::TranslationOnlyResponse>(
                prompt,
                shared::TRANSLATION_PREAMBLE,
            )
            .await?;

        Ok(ProviderTextResult {
            chapter: StructuredChapter {
                chapter_number: response.chapter_number,
                chapter_title: response.chapter_title,
                content: response.content,
            }
            .normalized(),
            usage: usage.into(),
        })
    }

    async fn extract_glossary(
        &self,
        req: GlossaryExtractionRequest,
    ) -> Result<ProviderGlossaryResult> {
        let prompt = build_glossary_extraction_prompt(&req);
        let (response, usage) = self
            .extract_structured::<shared::GlossaryExtractionResponse>(
                prompt,
                shared::GLOSSARY_PREAMBLE,
            )
            .await?;

        Ok(ProviderGlossaryResult {
            new_glossary_terms: response.new_glossary_terms,
            usage: usage.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_new_without_base_url_constructs() {
        let params = ProviderParams {
            api_key: "test-key-12345".to_string(),
            model: "gpt-4o-mini".to_string(),
        };
        let provider = OpenAiProvider::new(params, None);
        assert!(
            provider.is_ok(),
            "OpenAiProvider::new should succeed: {:?}",
            provider.err()
        );
    }

    #[test]
    fn openai_new_with_base_url_constructs() {
        let params = ProviderParams {
            api_key: "test-key-12345".to_string(),
            model: "gpt-4o-mini".to_string(),
        };
        let provider = OpenAiProvider::new(params, Some("https://api.example.com/v1"));
        assert!(
            provider.is_ok(),
            "OpenAiProvider::new with base_url should succeed: {:?}",
            provider.err()
        );
    }
}
