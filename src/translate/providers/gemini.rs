//! Gemini provider implementation.

use crate::error::{Error, Result};
use crate::translate::providers::ProviderParams;
use crate::translate::providers::shared::{self, HttpErrorMessages};
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
            kind: crate::config::ProviderKind::Gemini,
            detail: format!("Failed to build Gemini client: {e}"),
        })?;

        Ok(Self {
            client,
            model: params.model,
        })
    }
}

#[async_trait::async_trait]
impl shared::StructuredExtractor for GeminiProvider {
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
                        kind: crate::config::ProviderKind::Gemini,
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
