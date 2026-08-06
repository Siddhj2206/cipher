//! Cohere provider implementation.
//!
//! Cohere is reached through its OpenAI-compatible endpoint
//! (`/compatibility/v1`), so the shared OpenAI Chat Completions transport
//! applies. Cohere's compatibility layer rejects the `tool_choice` parameter
//! for some models (e.g. `command-a-plus`), so the custom extension below
//! keeps `tools` (the extractor's `submit` tool needs them) but strips
//! `tool_choice` from the serialized request body.

use crate::config::ProviderKind;
use crate::error::{Error, Result};
use crate::translate::providers::ProviderParams;
use crate::translate::providers::shared::{self, HttpErrorMessages};
use rig::client::{
    self, BearerAuth, Capabilities, Capable, CompletionClient, DebugExt, Nothing, Provider,
    ProviderBuilder,
};
use rig::completion::CompletionError;
use rig::http_client::{self, HttpClientExt};
use rig::providers::openai;
use serde::Serialize;
use serde::de::DeserializeOwned;

const COHERE_HTTP_MSGS: HttpErrorMessages = HttpErrorMessages {
    not_found: "Check your base URL and model name",
    unauthorized: "Check your Cohere API key",
    rate_limited: "Rate limit exceeded",
    server_error: "Provider issue",
};

const COHERE_COMPAT_ENDPOINT: &str = "https://api.cohere.ai/compatibility/v1";

/// Provider extension for Cohere's OpenAI-compatible endpoint.
#[derive(Debug, Default, Clone, Copy)]
pub struct CohereExt;

#[derive(Debug, Default, Clone, Copy)]
pub struct CohereBuilder;

type CohereApiKey = BearerAuth;

impl Provider for CohereExt {
    type Builder = CohereBuilder;

    const VERIFY_PATH: &'static str = "/models";
}

impl openai::completion::OpenAICompatibleProvider for CohereExt {
    const PROVIDER_NAME: &'static str = "cohere";

    type StreamingUsage = openai::Usage;
    type Response = openai::CompletionResponse;

    fn finalize_request_body(
        &self,
        body: &mut serde_json::Value,
    ) -> std::result::Result<(), CompletionError> {
        if let Some(obj) = body.as_object_mut() {
            obj.remove("tool_choice");
        }
        Ok(())
    }
}

impl<H> Capabilities<H> for CohereExt {
    type Completion = Capable<CompletionModel<H>>;
    type Transcription = Nothing;
    type Embeddings = Nothing;
    type ModelListing = Nothing;
    type Rerank = Nothing;
}

impl DebugExt for CohereExt {}

impl ProviderBuilder for CohereBuilder {
    type Extension<H>
        = CohereExt
    where
        H: HttpClientExt;
    type ApiKey = CohereApiKey;

    const BASE_URL: &'static str = COHERE_COMPAT_ENDPOINT;

    fn build<H>(
        _builder: &client::ClientBuilder<Self, Self::ApiKey, H>,
    ) -> http_client::Result<Self::Extension<H>>
    where
        H: HttpClientExt,
    {
        Ok(CohereExt)
    }
}

pub type Client = client::Client<CohereExt>;
pub type ClientBuilder = client::ClientBuilder<CohereBuilder, CohereApiKey>;

/// Cohere completion model, driven by the shared OpenAI Chat Completions path.
pub type CompletionModel<H> = openai::completion::GenericCompletionModel<CohereExt, H>;

/// Completion response, shared with the OpenAI Chat Completions path.
pub type CompletionResponse = openai::CompletionResponse;

pub struct CohereProvider {
    client: Client,
    model: String,
    endpoint: String,
}

impl CohereProvider {
    pub fn new(params: ProviderParams, base_url: Option<&str>) -> Result<Self> {
        let endpoint = base_url
            .map(str::to_string)
            .unwrap_or_else(|| COHERE_COMPAT_ENDPOINT.to_string());
        let client = Client::builder()
            .api_key(&params.api_key)
            .base_url(&endpoint)
            .build()
            .map_err(|e| Error::Provider {
                kind: ProviderKind::Cohere,
                detail: format!("Failed to build Cohere client: {e}"),
            })?;

        Ok(Self {
            client,
            model: params.model,
            endpoint,
        })
    }
}

#[async_trait::async_trait]
impl shared::StructuredExtractor for CohereProvider {
    async fn extract_structured<T>(
        &self,
        operation: &str,
        prompt: String,
        preamble: &str,
    ) -> Result<(T, rig::completion::Usage)>
    where
        T: DeserializeOwned + Serialize + schemars::JsonSchema + Send + Sync + 'static,
    {
        shared::tracked_call(
            operation,
            ProviderKind::Cohere.slug(),
            &self.model,
            &self.endpoint,
            async {
                let extractor = self
                    .client
                    .extractor::<T>(&self.model)
                    .preamble(preamble)
                    .retries(shared::EXTRACTOR_RETRIES)
                    .build();

                match extractor.extract_with_usage(&prompt).await {
                    Ok(extracted) => Ok((extracted.data, extracted.usage)),
                    Err(err) => {
                        let detailed_error =
                            shared::format_extraction_error(&err, &COHERE_HTTP_MSGS);
                        Err(Error::Provider {
                            kind: ProviderKind::Cohere,
                            detail: detailed_error,
                        })
                    }
                }
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::providers::openai::OpenAICompatibleProvider;

    #[test]
    fn cohere_new_constructs_without_base_url() {
        let params = ProviderParams {
            api_key: "test-key-12345".to_string(),
            model: "command-a-plus-05-2026".to_string(),
        };
        let provider = CohereProvider::new(params, None);
        assert!(
            provider.is_ok(),
            "CohereProvider::new should succeed: {:?}",
            provider.err()
        );
    }

    #[test]
    fn cohere_new_constructs_with_base_url() {
        let params = ProviderParams {
            api_key: "test-key-12345".to_string(),
            model: "command-a-plus-05-2026".to_string(),
        };
        let provider = CohereProvider::new(params, Some(COHERE_COMPAT_ENDPOINT));
        assert!(
            provider.is_ok(),
            "CohereProvider::new with base_url should succeed: {:?}",
            provider.err()
        );
    }

    #[test]
    fn cohere_finalize_request_body_strips_tool_choice() {
        let mut body = serde_json::json!({
            "model": "command-a-plus-05-2026",
            "messages": [{"role": "user", "content": "Hello"}],
            "tools": [{"type": "function", "function": {"name": "submit"}}],
            "tool_choice": "required"
        });

        CohereExt
            .finalize_request_body(&mut body)
            .expect("finalize should succeed");

        assert!(body.get("tool_choice").is_none());
        assert!(body.get("tools").is_some());
    }

    #[test]
    fn cohere_finalize_request_body_preserves_body_without_tool_choice() {
        let mut body = serde_json::json!({
            "model": "command-a-plus-05-2026",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        CohereExt
            .finalize_request_body(&mut body)
            .expect("finalize should succeed");

        assert!(body.get("tool_choice").is_none());
        assert_eq!(body["model"], "command-a-plus-05-2026");
    }
}
