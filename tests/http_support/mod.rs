use serde_json::{Value, json};
use wiremock::{Request, Respond, ResponseTemplate};

pub struct StructuredReply;

impl Respond for StructuredReply {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = request.body_json().unwrap();
        let extraction = body["messages"]
            .to_string()
            .contains("You extract glossary terms");
        let data = if extraction {
            json!({"new_glossary_terms": []})
        } else {
            json!({"chapter_number": "1", "chapter_title": "Translated", "content": "Translated body"})
        };
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "mock-completion", "object": "chat.completion", "created": 1,
            "model": "test-model",
            "choices": [{"index": 0, "finish_reason": "tool_calls", "message": {
                "role": "assistant", "content": null,
                "tool_calls": [{"id": "call-1", "type": "function", "function": {
                    "name": "submit", "arguments": data.to_string()
                }}]
            }}],
            "usage": {"prompt_tokens": 12, "completion_tokens": 7, "total_tokens": 19}
        }))
    }
}
