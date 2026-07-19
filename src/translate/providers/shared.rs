use rig::completion::CompletionError;
use rig::extractor::ExtractionError;
use serde::{Deserialize, Serialize};

use crate::glossary::GlossaryTerm;

pub const EXTRACTOR_RETRIES: u64 = 1;
pub const TRANSLATION_PREAMBLE: &str =
    "You are a professional translator. Always return valid JSON matching the expected schema.";
pub const GLOSSARY_PREAMBLE: &str =
    "You extract glossary terms. Always return valid JSON matching the expected schema.";

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TranslationOnlyResponse {
    pub chapter_number: Option<String>,
    pub chapter_title: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct GlossaryExtractionResponse {
    pub new_glossary_terms: Vec<GlossaryTerm>,
}

pub struct HttpErrorMessages {
    pub not_found: &'static str,
    pub unauthorized: &'static str,
    pub rate_limited: &'static str,
    pub server_error: &'static str,
}

pub fn format_completion_error(err: &CompletionError, http_msgs: &HttpErrorMessages) -> String {
    match err {
        CompletionError::HttpError(http_err) => {
            let err_str = format!("{}", http_err);
            if err_str.contains("404") {
                format!("HTTP 404: Not Found - {}", http_msgs.not_found)
            } else if err_str.contains("401") || err_str.contains("403") {
                format!("HTTP 401/403: Unauthorized - {}", http_msgs.unauthorized)
            } else if err_str.contains("429") {
                format!("HTTP 429: Too Many Requests - {}", http_msgs.rate_limited)
            } else if err_str.contains("500") {
                format!(
                    "HTTP 500: Internal Server Error - {}",
                    http_msgs.server_error
                )
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

pub fn format_extraction_error(err: &ExtractionError, http_msgs: &HttpErrorMessages) -> String {
    match err {
        ExtractionError::NoData => "No data extracted".to_string(),
        ExtractionError::DeserializationError(json_err) => {
            format!("JSON deserialization error: {}", json_err)
        }
        ExtractionError::CompletionError(comp_err) => format_completion_error(comp_err, http_msgs),
    }
}
