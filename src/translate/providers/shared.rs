use rig::completion::CompletionError;
use rig::extractor::ExtractionError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::book::StructuredChapter;
use crate::error::Result;
use crate::glossary::GlossaryTerm;
use crate::output::verbose_detail_kv;
use crate::translate::prompt::{
    build_glossary_extraction_prompt, build_glossary_section, build_repair_prompt,
    build_style_section, build_translation_prompt,
};
use crate::translate::providers::Provider;
use crate::translate::{
    GlossaryExtractionRequest, ProviderGlossaryResult, ProviderTextResult, RepairRequest,
    TranslationRequest,
};
use std::future::Future;
use std::time::Instant;

pub const EXTRACTOR_RETRIES: u64 = 1;
pub const TRANSLATION_PREAMBLE: &str =
    "You are a professional translator. Always return valid JSON matching the expected schema.";
pub const GLOSSARY_PREAMBLE: &str =
    "You extract glossary terms. Always return valid JSON matching the expected schema.";

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct TranslationOnlyResponse {
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub chapter_number: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub chapter_title: Option<String>,
    pub content: String,
}

/// Parse an optional string field leniently: some models emit numbers or
/// bools (e.g. Cohere's `"chapter_number": 2`); the JSON schema still asks
/// for strings, this only relaxes parsing of the reply.
fn deserialize_optional_string<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|v| match v {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }))
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
    // The provider's raw HTTP status (when the error carried one) drives the
    // user-facing message; matching on the code avoids string-sniffing the
    // rendered error text.
    if let Some(status) = err.provider_response_status() {
        return match status.as_u16() {
            404 => format!("HTTP 404: Not Found - {}", http_msgs.not_found),
            401 | 403 => format!("HTTP 401/403: Unauthorized - {}", http_msgs.unauthorized),
            429 => format!("HTTP 429: Too Many Requests - {}", http_msgs.rate_limited),
            500 => format!(
                "HTTP 500: Internal Server Error - {}",
                http_msgs.server_error
            ),
            _ => format!("HTTP error: {}", err),
        };
    }

    match err {
        CompletionError::HttpError(http_err) => format!("HTTP error: {}", http_err),
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

fn text_result(
    response: TranslationOnlyResponse,
    usage: rig::completion::Usage,
) -> ProviderTextResult {
    ProviderTextResult {
        chapter: StructuredChapter {
            chapter_number: response.chapter_number,
            chapter_title: response.chapter_title,
            content: response.content,
        }
        .normalized(),
        usage: usage.into(),
    }
}

/// Run a translation via a provider-specific structured extractor and shape
/// the response into a [`ProviderTextResult`].
pub(crate) async fn translate_via<F, Fut>(
    req: TranslationRequest,
    extract: F,
) -> Result<ProviderTextResult>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<(TranslationOnlyResponse, rig::completion::Usage)>>,
{
    let (response, usage) = extract(build_translation_prompt(&req)).await?;
    Ok(text_result(response, usage))
}

/// Run a repair via a provider-specific structured extractor and shape the
/// response into a [`ProviderTextResult`].
pub(crate) async fn repair_via<F, Fut>(req: RepairRequest, extract: F) -> Result<ProviderTextResult>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<(TranslationOnlyResponse, rig::completion::Usage)>>,
{
    let glossary_section = build_glossary_section(&req.glossary_terms);
    let style_section = build_style_section(&req.style_guide);
    let prompt = build_repair_prompt(&req, &glossary_section, &style_section);
    let (response, usage) = extract(prompt).await?;
    Ok(text_result(response, usage))
}

/// Run a glossary extraction via a provider-specific structured extractor and
/// shape the response into a [`ProviderGlossaryResult`].
pub(crate) async fn extract_glossary_via<F, Fut>(
    req: GlossaryExtractionRequest,
    extract: F,
) -> Result<ProviderGlossaryResult>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<(GlossaryExtractionResponse, rig::completion::Usage)>>,
{
    let (response, usage) = extract(build_glossary_extraction_prompt(&req)).await?;
    Ok(ProviderGlossaryResult {
        new_glossary_terms: response.new_glossary_terms,
        usage: usage.into(),
    })
}

/// The provider-specific structured-extraction capability that the shared
/// [`Provider`] operations are built on. Implementing this trait gives a
/// provider the shared `translate`/`repair`/`extract_glossary` operations for
/// free.
#[async_trait::async_trait]
pub(crate) trait StructuredExtractor: Send + Sync {
    async fn extract_structured<T>(
        &self,
        operation: &str,
        prompt: String,
        preamble: &str,
    ) -> Result<(T, rig::completion::Usage)>
    where
        T: DeserializeOwned + Serialize + schemars::JsonSchema + Send + Sync + 'static;
}

/// Blanket implementation: any [`StructuredExtractor`] is a full [`Provider`].
#[async_trait::async_trait]
impl<P> Provider for P
where
    P: StructuredExtractor + Send + Sync + 'static,
{
    async fn translate(&self, req: TranslationRequest) -> Result<ProviderTextResult> {
        let extract = |prompt: String| {
            self.extract_structured::<TranslationOnlyResponse>(
                "translate",
                prompt,
                TRANSLATION_PREAMBLE,
            )
        };
        translate_via(req, extract).await
    }

    async fn repair(&self, req: RepairRequest) -> Result<ProviderTextResult> {
        let extract = |prompt: String| {
            self.extract_structured::<TranslationOnlyResponse>(
                "repair",
                prompt,
                TRANSLATION_PREAMBLE,
            )
        };
        repair_via(req, extract).await
    }

    async fn extract_glossary(
        &self,
        req: GlossaryExtractionRequest,
    ) -> Result<ProviderGlossaryResult> {
        let extract = |prompt: String| {
            self.extract_structured::<GlossaryExtractionResponse>(
                "glossary",
                prompt,
                GLOSSARY_PREAMBLE,
            )
        };
        extract_glossary_via(req, extract).await
    }
}

/// Run a provider call under verbose diagnostics: log the call details
/// (operation, provider kind, model, endpoint) before the call and the
/// outcome with wall-clock duration after it. The result passes through
/// unchanged.
pub(crate) async fn tracked_call<T>(
    operation: &str,
    kind: &str,
    model: &str,
    endpoint: &str,
    call: impl Future<Output = Result<T>>,
) -> Result<T> {
    verbose_detail_kv(
        "Provider call",
        format!("{operation}: {kind} (model={model}, endpoint={endpoint})"),
    );
    let start = Instant::now();
    let result = call.await;
    let elapsed_ms = start.elapsed().as_millis();
    match &result {
        Ok(_) => verbose_detail_kv(
            "Provider call",
            format!("{operation}: ok in {elapsed_ms} ms"),
        ),
        Err(e) => verbose_detail_kv(
            "Provider call",
            format!("{operation}: failed after {elapsed_ms} ms: {e}"),
        ),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::output::set_verbose;

    #[tokio::test]
    async fn tracked_call_passes_through_ok() {
        set_verbose(false);
        let result = tracked_call("translate", "gemini", "m", "https://example.com", async {
            Ok::<_, Error>(42)
        })
        .await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn tracked_call_passes_through_error() {
        set_verbose(false);
        let result = tracked_call("repair", "openai", "m", "https://example.com", async {
            Err::<u32, _>(Error::Provider {
                kind: crate::config::ProviderKind::Openai,
                detail: "boom".to_string(),
            })
        })
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tracked_call_logs_without_panicking_when_verbose() {
        set_verbose(true);
        let _ = tracked_call("translate", "gemini", "m", "https://example.com", async {
            Ok::<_, Error>("ok")
        })
        .await;
        let _ = tracked_call("glossary", "openai", "m", "https://example.com", async {
            Err::<u32, _>(Error::Provider {
                kind: crate::config::ProviderKind::Openai,
                detail: "boom".to_string(),
            })
        })
        .await;
        set_verbose(false);
    }

    #[test]
    fn translation_response_tolerates_numeric_metadata() {
        let json = r#"{
            "chapter_number": 2,
            "chapter_title": "Chapter 2",
            "content": "hello"
        }"#;
        let parsed: TranslationOnlyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.chapter_number.as_deref(), Some("2"));
        assert_eq!(parsed.chapter_title.as_deref(), Some("Chapter 2"));
        assert_eq!(parsed.content, "hello");
    }

    #[test]
    fn translation_response_tolerates_bool_and_missing_metadata() {
        let json = r#"{"chapter_title": true, "content": "hi"}"#;
        let parsed: TranslationOnlyResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.chapter_number, None);
        assert_eq!(parsed.chapter_title.as_deref(), Some("true"));
        assert_eq!(parsed.content, "hi");
    }

    #[test]
    fn translation_response_rejects_structured_content() {
        let json = r#"{"content": {"nested": true}}"#;
        assert!(serde_json::from_str::<TranslationOnlyResponse>(json).is_err());
    }
}
