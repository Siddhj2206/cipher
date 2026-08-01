//! OpenAI provider implementation
//!
//! Supports both OpenAI and OpenAI-compatible endpoints
//! - OpenAI: Uses Responses API (best structured output support)
//! - OpenAI-compatible: Uses Chat Completions API (more widely supported)

use crate::error::{Error, Result};
use crate::translate::providers::ProviderParams;
use crate::translate::providers::shared::{self, HttpErrorMessages};
use rig::providers::openai;
use serde::Serialize;
use serde::de::DeserializeOwned;

const OPENAI_HTTP_MSGS: HttpErrorMessages = HttpErrorMessages {
    not_found: "Check your base URL and model name",
    unauthorized: "Check your API key",
    rate_limited: "Rate limit exceeded",
    server_error: "Provider issue",
};

const OPENAI_ENDPOINT: &str = "https://api.openai.com/v1";

pub struct OpenAiProvider {
    client: openai::Client,
    model: String,
    endpoint: String,
    use_completions_api: bool,
}

impl OpenAiProvider {
    pub fn new(params: ProviderParams, base_url: Option<&str>) -> Result<Self> {
        let client = if let Some(url) = base_url {
            openai::Client::builder()
                .api_key(&params.api_key)
                .base_url(url)
                .build()
                .map_err(|e| Error::Provider {
                    kind: crate::config::ProviderKind::Openai,
                    detail: format!("Failed to build OpenAI client: {e}"),
                })?
        } else {
            openai::Client::new(&params.api_key).map_err(|e| Error::Provider {
                kind: crate::config::ProviderKind::Openai,
                detail: format!("Failed to build OpenAI client: {e}"),
            })?
        };

        let endpoint = base_url
            .map(str::to_string)
            .unwrap_or_else(|| OPENAI_ENDPOINT.to_string());
        let use_completions_api = base_url.is_some();

        Ok(Self {
            client,
            model: params.model,
            endpoint,
            use_completions_api,
        })
    }
}

#[async_trait::async_trait]
impl shared::StructuredExtractor for OpenAiProvider {
    async fn extract_structured<T>(
        &self,
        operation: &str,
        prompt: String,
        preamble: &str,
    ) -> Result<(T, rig::completion::Usage)>
    where
        T: DeserializeOwned + Serialize + schemars::JsonSchema + Send + Sync + 'static,
    {
        shared::tracked_call(operation, "openai", &self.model, &self.endpoint, async {
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
                    Err(Error::Provider {
                        kind: crate::config::ProviderKind::Openai,
                        detail: detailed_error,
                    })
                }
            }
        })
        .await
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
