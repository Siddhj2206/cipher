//! Gemini provider implementation.

use anyhow::Result;
use rig::client::CompletionClient;
use rig::completion::CompletionError;
use rig::extractor::ExtractionError;
use rig::providers::gemini;

use crate::translate::prompt::build_prompt;
use crate::translate::providers::{Provider, ProviderParams};
use crate::translate::{TranslationRequest, TranslationResponse};

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
    async fn translate(&self, req: TranslationRequest) -> Result<TranslationResponse> {
        let prompt = build_prompt(&req);
        let extractor = self
            .client
            .extractor::<TranslationResponse>(&self.model)
            .preamble(
                "You are a professional translator. Always return valid JSON matching the TranslationResponse schema.",
            )
            .build();

        match extractor.extract(&prompt).await {
            Ok(response) => Ok(response),
            Err(err) => {
                let detailed_error = format_extraction_error(&err);
                Err(anyhow::anyhow!("LLM request failed: {}", detailed_error))
            }
        }
    }
}
