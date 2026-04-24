//! Gemini provider implementation.

use anyhow::Result;
use rig::client::CompletionClient;
use rig::completion::CompletionError;
use rig::extractor::ExtractionError;
use rig::providers::gemini;
use serde::{Deserialize, Serialize};

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
    translation: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
struct GlossaryExtractionResponse {
    new_glossary_terms: Vec<GlossaryTerm>,
}

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

fn format_completion_error(err: &CompletionError) -> String {
    match err {
        CompletionError::HttpError(http_err) => {
            let err_str = format!("{}", http_err);
            if err_str.contains("404") {
                "HTTP 404: Not Found - Check your model name".to_string()
            } else if err_str.contains("401") || err_str.contains("403") {
                "HTTP 401/403: Unauthorized - Check your Gemini API key".to_string()
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
impl Provider for GeminiProvider {
    async fn translate(&self, req: TranslationRequest) -> Result<ProviderTextResult> {
        let prompt = build_translation_prompt(&req);
        let extractor = self
            .client
            .extractor::<TranslationOnlyResponse>(&self.model)
            .preamble(TRANSLATION_PREAMBLE)
            .retries(EXTRACTOR_RETRIES)
            .build();

        match extractor.extract_with_usage(&prompt).await {
            Ok(extracted) => Ok(ProviderTextResult {
                text: extracted.data.translation,
                usage: extracted.usage.into(),
            }),
            Err(err) => {
                let detailed_error = format_extraction_error(&err);
                Err(anyhow::anyhow!("LLM request failed: {}", detailed_error))
            }
        }
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
        let extractor = self
            .client
            .extractor::<TranslationOnlyResponse>(&self.model)
            .preamble(TRANSLATION_PREAMBLE)
            .retries(EXTRACTOR_RETRIES)
            .build();

        match extractor.extract_with_usage(&prompt).await {
            Ok(extracted) => Ok(ProviderTextResult {
                text: extracted.data.translation,
                usage: extracted.usage.into(),
            }),
            Err(err) => {
                let detailed_error = format_extraction_error(&err);
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
            .extractor::<GlossaryExtractionResponse>(&self.model)
            .preamble(GLOSSARY_PREAMBLE)
            .retries(EXTRACTOR_RETRIES)
            .build();

        match extractor.extract_with_usage(&prompt).await {
            Ok(extracted) => Ok(ProviderGlossaryResult {
                new_glossary_terms: extracted.data.new_glossary_terms,
                usage: extracted.usage.into(),
            }),
            Err(err) => {
                let detailed_error = format_extraction_error(&err);
                Err(anyhow::anyhow!("LLM request failed: {}", detailed_error))
            }
        }
    }
}
