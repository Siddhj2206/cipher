use cipher::book::OutputConfig;
use cipher::config::ProviderKind;
use cipher::error::Error;
use cipher::translate::Translator;
use cipher::translate::providers::{
    Provider, ProviderParams, cohere::CohereProvider, openai::OpenAiProvider,
};
use serde_json::Value;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod http_support;

fn provider(kind: &ProviderKind, url: &str) -> Box<dyn Provider> {
    let params = ProviderParams {
        api_key: "test-key".into(),
        model: "test-model".into(),
    };
    match kind {
        ProviderKind::Cohere => Box::new(CohereProvider::new(params, Some(url)).unwrap()),
        ProviderKind::Openai => Box::new(OpenAiProvider::new(params, Some(url)).unwrap()),
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn compatible_providers_send_structured_requests_and_parse_responses() {
    for kind in [ProviderKind::Openai, ProviderKind::Cohere] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(http_support::StructuredReply)
            .expect(2)
            .mount(&server)
            .await;
        let translator = Translator::new(provider(&kind, &format!("{}/v1", server.uri())));
        let result = translator
            .translate_chapter(
                "Raw chapter sentinel",
                &[],
                Some("Style sentinel".into()),
                OutputConfig::default(),
            )
            .await
            .unwrap();
        assert_eq!(result.chapter.content, "Translated body");
        assert_eq!(result.chapter.chapter_number.as_deref(), Some("1"));
        assert_eq!(result.usage.input_tokens, 12);
        assert_eq!(result.usage.output_tokens, 7);
        let terms = translator
            .extract_glossary("Raw chapter sentinel", "Translated body".into(), &[])
            .await
            .unwrap();
        assert!(terms.new_glossary_terms.is_empty());
        let requests = server.received_requests().await.unwrap();
        let body: Value = requests[0].body_json().unwrap();
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["tools"][0]["function"]["name"], "submit");
        assert!(
            body["tools"][0]["function"]["parameters"]["properties"]
                .get("content")
                .is_some()
        );
        assert!(
            body["messages"]
                .to_string()
                .contains("Raw chapter sentinel")
        );
        assert!(body["messages"].to_string().contains("Style sentinel"));
        if kind == ProviderKind::Cohere {
            assert!(body.get("tool_choice").is_none());
        }
        server.verify().await;
    }
}

#[tokio::test]
async fn compatible_providers_map_http_unauthorized_to_typed_provider_errors() {
    for kind in [ProviderKind::Openai, ProviderKind::Cohere] {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": {"message": "invalid key", "type": "authentication_error"}})))
            .expect(2).mount(&server).await;
        let translator = Translator::new(provider(&kind, &format!("{}/v1", server.uri())));
        let error = translator
            .translate_chapter("Raw", &[], None, OutputConfig::default())
            .await
            .unwrap_err();
        match error {
            Error::Provider {
                kind: actual,
                detail,
            } => {
                assert_eq!(actual, kind);
                assert!(detail.contains("HTTP 401/403: Unauthorized"), "{detail}");
                assert!(detail.contains("API key"), "{detail}");
            }
            other => panic!("expected provider error, got {other:?}"),
        }
        server.verify().await;
    }
}
