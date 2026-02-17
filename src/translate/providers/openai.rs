//! OpenAI provider implementation
//!
//! Supports both OpenAI and OpenAI-compatible endpoints
//! - OpenAI: Uses Responses API (best structured output support)
//! - OpenAI-compatible: Uses Chat Completions API (more widely supported)

use crate::translate::prompt::build_prompt;
use crate::translate::providers::{Provider, ProviderParams};
use crate::translate::{TranslationRequest, TranslationResponse};
use anyhow::Result;
use rig::completion::CompletionError;
use rig::extractor::ExtractionError;
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

fn format_completion_error(err: &CompletionError) -> String {
    match err {
        CompletionError::HttpError(http_err) => {
            // Format the HTTP error to show status code if available
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
        _ => {
            format!("Unknown error: {:?}", err)
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
    async fn translate(&self, req: TranslationRequest) -> Result<TranslationResponse> {
        let prompt = build_prompt(&req);

        let result = if self.use_completions_api {
            // Use Chat Completions API (for OpenAI-compatible endpoints)
            let completions_client = self.client.clone().completions_api();
            let extractor = completions_client
                .extractor::<TranslationResponse>(&self.model)
                .preamble("You are a professional translator. Always return valid JSON matching the TranslationResponse schema.")
                .build();

            extractor.extract(&prompt).await
        } else {
            // Use Responses API (for real OpenAI - best structured output support)
            let extractor = self
                .client
                .extractor::<TranslationResponse>(&self.model)
                .preamble("You are a professional translator. Always return valid JSON matching the TranslationResponse schema.")
                .build();

            extractor.extract(&prompt).await
        };

        match result {
            Ok(response) => Ok(response),
            Err(err) => {
                let detailed_error = format_extraction_error(&err);
                Err(anyhow::anyhow!("LLM request failed: {}", detailed_error))
            }
        }
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
