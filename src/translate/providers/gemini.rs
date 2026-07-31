//! Gemini provider implementation.

use crate::book::StructuredChapter;
use crate::error::{Error, Result};
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
use rig::client::CompletionClient;
use rig::providers::gemini;
use serde::Serialize;
use serde::de::DeserializeOwned;

const GEMINI_HTTP_MSGS: HttpErrorMessages = HttpErrorMessages {
    not_found: "Check your model name",
    unauthorized: "Check your Gemini API key",
    rate_limited: "Rate limit exceeded",
    server_error: "Provider issue",
};

const GEMINI_ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct GeminiProvider {
    client: gemini::Client,
    model: String,
}

impl GeminiProvider {
    pub fn new(params: ProviderParams) -> Result<Self> {
        let client = gemini::Client::new(&params.api_key).map_err(|e| Error::Provider {
            kind: "gemini".to_string(),
            detail: format!("Failed to build Gemini client: {e}"),
        })?;

        Ok(Self {
            client,
            model: params.model,
        })
    }

    async fn extract_structured<T>(
        &self,
        operation: &str,
        prompt: String,
        preamble: &str,
    ) -> Result<(T, rig::completion::Usage)>
    where
        T: DeserializeOwned + Serialize + schemars::JsonSchema + Send + Sync + 'static,
    {
        shared::tracked_call(operation, "gemini", &self.model, GEMINI_ENDPOINT, async {
            let extractor = self
                .client
                .extractor::<T>(&self.model)
                .preamble(preamble)
                .retries(shared::EXTRACTOR_RETRIES)
                .build();

            match extractor.extract_with_usage(&prompt).await {
                Ok(extracted) => Ok((extracted.data, extracted.usage)),
                Err(err) => {
                    let detailed_error = shared::format_extraction_error(&err, &GEMINI_HTTP_MSGS);
                    Err(Error::Provider {
                        kind: "gemini".to_string(),
                        detail: detailed_error,
                    })
                }
            }
        })
        .await
    }
}

#[async_trait::async_trait]
impl Provider for GeminiProvider {
    async fn translate(&self, req: TranslationRequest) -> Result<ProviderTextResult> {
        let prompt = build_translation_prompt(&req);
        let (response, usage) = self
            .extract_structured::<shared::TranslationOnlyResponse>(
                "translate",
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
                "repair",
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
                "glossary",
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
    fn gemini_new_constructs() {
        let params = ProviderParams {
            api_key: "test-key-12345".to_string(),
            model: "gemini-2.5-flash".to_string(),
        };
        let provider = GeminiProvider::new(params);
        assert!(
            provider.is_ok(),
            "GeminiProvider::new should succeed: {:?}",
            provider.err()
        );
    }
}
