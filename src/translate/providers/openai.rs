//! OpenAI provider implementation
//!
//! Supports both OpenAI and OpenAI-compatible endpoints
//! - OpenAI: Uses Responses API (best structured output support)
//! - OpenAI-compatible: Uses Chat Completions API (more widely supported)

use anyhow::Result;
use rig::completion::CompletionError;
use rig::extractor::ExtractionError;
use rig::providers::openai;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::book::StructuredChapter;
use crate::glossary::GlossaryTerm;
use crate::translate::prompt::{
    build_glossary_extraction_prompt, build_repair_prompt, build_translation_prompt,
};
use crate::translate::providers::{Provider, ProviderParams};
use crate::translate::{
    GlossaryExtractionRequest, ProviderGlossaryResult, ProviderTextResult, RepairRequest,
    TranslationRequest,
};

const EXTRACTOR_RETRIES: u64 = 1;
const TRANSLATION_PREAMBLE: &str =
    "You are a professional translator. Always return valid JSON matching the expected schema.";
const GLOSSARY_PREAMBLE: &str =
    "You extract glossary terms. Always return valid JSON matching the expected schema.";

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
struct TranslationOnlyResponse {
    chapter_number: Option<String>,
    chapter_title: Option<String>,
    content: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
struct GlossaryExtractionResponse {
    new_glossary_terms: Vec<GlossaryTerm>,
}

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
                .retries(EXTRACTOR_RETRIES)
                .build();

            extractor.extract_with_usage(&prompt).await
        } else {
            let extractor = self
                .client
                .extractor::<T>(&self.model)
                .preamble(preamble)
                .retries(EXTRACTOR_RETRIES)
                .build();

            extractor.extract_with_usage(&prompt).await
        };

        match result {
            Ok(extracted) => Ok((extracted.data, extracted.usage)),
            Err(err) => {
                let detailed_error = format_extraction_error(&err);
                Err(anyhow::anyhow!("LLM request failed: {}", detailed_error))
            }
        }
    }
}

fn format_completion_error(err: &CompletionError) -> String {
    match err {
        CompletionError::HttpError(http_err) => {
            let err_str = format!("{}", http_err);
            if err_str.contains("404") {
                "HTTP 404: Not Found - Check your base URL and model name".to_string()
            } else if err_str.contains("401") {
                "HTTP 401: Unauthorized - Check your API key".to_string()
            } else if err_str.contains("429") {
                "HTTP 429: Too Many Requests - Rate limit exceeded".to_string()
            } else if err_str.contains("500") {
                "HTTP 500: Internal Server Error - Provider issue".to_string()
            } else {
                format!("HTTP error: {}", err_str)
            }
        }
        CompletionError::JsonError(json_err) => {
            format!("JSON parsing error: {}", json_err)
        }
        CompletionError::RequestError(req_err) => {
            format!("Request error: {}", req_err)
        }
        CompletionError::ResponseError(resp) => {
            format!("Provider response error: {}", resp)
        }
        CompletionError::ProviderError(msg) => {
            format!("Provider error: {}", msg)
        }
        other => {
            format!(
                "API error: {} (if this persists, please report as a bug)",
                other
            )
        }
    }
}

fn format_extraction_error(err: &ExtractionError) -> String {
    match err {
        ExtractionError::NoData => "No data extracted".to_string(),
        ExtractionError::DeserializationError(json_err) => {
            format!("JSON deserialization error: {}", json_err)
        }
        ExtractionError::CompletionError(comp_err) => format_completion_error(comp_err),
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiProvider {
    async fn translate(&self, req: TranslationRequest) -> Result<ProviderTextResult> {
        let prompt = build_translation_prompt(&req);
        let (response, usage) = self
            .extract_structured::<TranslationOnlyResponse>(prompt, TRANSLATION_PREAMBLE)
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
        let glossary_section = if req.glossary_terms.is_empty() {
            "(No glossary terms available)".to_string()
        } else {
            req.glossary_terms
                .iter()
                .map(|t| {
                    if let Some(ref og) = t.og_term {
                        format!("{} [{}]: {}", t.term, og, t.definition)
                    } else {
                        format!("{}: {}", t.term, t.definition)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let style_section = match &req.style_guide {
            Some(guide) if !guide.trim().is_empty() => format!(
                r#"

**Style Guide:**

Follow these additional style and tone instructions carefully:

{}
"#,
                guide.trim()
            ),
            _ => String::new(),
        };
        let prompt = build_repair_prompt(&req, &glossary_section, &style_section);
        let (response, usage) = self
            .extract_structured::<TranslationOnlyResponse>(prompt, TRANSLATION_PREAMBLE)
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
            .extract_structured::<GlossaryExtractionResponse>(prompt, GLOSSARY_PREAMBLE)
            .await?;

        Ok(ProviderGlossaryResult {
            new_glossary_terms: response.new_glossary_terms,
            usage: usage.into(),
        })
    }
}
