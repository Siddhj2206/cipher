//! Gemini provider implementation.

use anyhow::Result;
use rig::client::CompletionClient;
use rig::providers::gemini;

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

const GEMINI_HTTP_MSGS: HttpErrorMessages = HttpErrorMessages {
    not_found: "Check your model name",
    unauthorized: "Check your Gemini API key",
    rate_limited: "Rate limit exceeded",
    server_error: "Provider issue",
};

pub struct GeminiProvider {
    client: gemini::Client,
    model: String,
}

impl GeminiProvider {
    pub fn new(params: ProviderParams) -> Result<Self> {
        let client = gemini::Client::new(&params.api_key)
            .map_err(|e| anyhow::anyhow!("Failed to build Gemini client: {}", e))?;

        Ok(Self {
            client,
            model: params.model,
        })
    }
}

#[async_trait::async_trait]
impl Provider for GeminiProvider {
    async fn translate(&self, req: TranslationRequest) -> Result<ProviderTextResult> {
        let prompt = build_translation_prompt(&req);
        let extractor = self
            .client
            .extractor::<shared::TranslationOnlyResponse>(&self.model)
            .preamble(shared::TRANSLATION_PREAMBLE)
            .retries(shared::EXTRACTOR_RETRIES)
            .build();

        match extractor.extract_with_usage(&prompt).await {
            Ok(extracted) => Ok(ProviderTextResult {
                chapter: StructuredChapter {
                    chapter_number: extracted.data.chapter_number,
                    chapter_title: extracted.data.chapter_title,
                    content: extracted.data.content,
                }
                .normalized(),
                usage: extracted.usage.into(),
            }),
            Err(err) => {
                let detailed_error = shared::format_extraction_error(&err, &GEMINI_HTTP_MSGS);
                Err(anyhow::anyhow!("LLM request failed: {}", detailed_error))
            }
        }
    }

    async fn repair(&self, req: RepairRequest) -> Result<ProviderTextResult> {
        let glossary_section = build_glossary_section(&req.glossary_terms);
        let style_section = build_style_section(&req.style_guide);
        let prompt = build_repair_prompt(&req, &glossary_section, &style_section);
        let extractor = self
            .client
            .extractor::<shared::TranslationOnlyResponse>(&self.model)
            .preamble(shared::TRANSLATION_PREAMBLE)
            .retries(shared::EXTRACTOR_RETRIES)
            .build();

        match extractor.extract_with_usage(&prompt).await {
            Ok(extracted) => Ok(ProviderTextResult {
                chapter: StructuredChapter {
                    chapter_number: extracted.data.chapter_number,
                    chapter_title: extracted.data.chapter_title,
                    content: extracted.data.content,
                }
                .normalized(),
                usage: extracted.usage.into(),
            }),
            Err(err) => {
                let detailed_error = shared::format_extraction_error(&err, &GEMINI_HTTP_MSGS);
                Err(anyhow::anyhow!("LLM request failed: {}", detailed_error))
            }
        }
    }

    async fn extract_glossary(
        &self,
        req: GlossaryExtractionRequest,
    ) -> Result<ProviderGlossaryResult> {
        let prompt = build_glossary_extraction_prompt(&req);
        let extractor = self
            .client
            .extractor::<shared::GlossaryExtractionResponse>(&self.model)
            .preamble(shared::GLOSSARY_PREAMBLE)
            .retries(shared::EXTRACTOR_RETRIES)
            .build();

        match extractor.extract_with_usage(&prompt).await {
            Ok(extracted) => Ok(ProviderGlossaryResult {
                new_glossary_terms: extracted.data.new_glossary_terms,
                usage: extracted.usage.into(),
            }),
            Err(err) => {
                let detailed_error = shared::format_extraction_error(&err, &GEMINI_HTTP_MSGS);
                Err(anyhow::anyhow!("LLM request failed: {}", detailed_error))
            }
        }
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
