//! ChatCompletions against a local stub server serving canned bytes.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cipher_llm::{
    ChatCompletions, Effort, Endpoint, InMemory, LlmError, Model, Protocol, Request, Secrets,
    Timeouts, Usage, connect,
};
use serde_json::Value;

/// Serves one canned response per connection, in order, and records each request.
struct Stub {
    base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
}

#[derive(Clone)]
struct Recorded {
    headers: Vec<(String, String)>,
    body: Value,
}

impl Stub {
    fn start(responses: Vec<Vec<u8>>) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        thread::spawn(move || {
            for response in responses {
                let Ok((mut conn, _)) = listener.accept() else {
                    return;
                };
                let recorded = read_request(&mut conn);
                log.lock().unwrap().push(recorded);
                let _ = conn.write_all(&response);
                let _ = conn.flush();
                // Dropping the connection closes it.
            }
        });
        Stub { base_url, requests }
    }

    fn last_request(&self) -> Recorded {
        self.requests.lock().unwrap().last().cloned().unwrap()
    }
}

fn read_request(conn: &mut TcpStream) -> Recorded {
    let mut reader = BufReader::new(conn.try_clone().unwrap());
    let mut headers = Vec::new();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap(); // request line
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.to_lowercase(), v.trim().to_owned()));
        }
    }
    let len: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .map(|(_, v)| v.parse().unwrap())
        .unwrap_or(0);
    let mut body = vec![0; len];
    reader.read_exact(&mut body).unwrap();
    Recorded {
        headers,
        body: serde_json::from_slice(&body).unwrap(),
    }
}

/// A 200 SSE response whose body ends when the connection closes.
fn sse(events: &[&str]) -> Vec<u8> {
    let mut out =
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n".to_vec();
    for e in events {
        out.extend_from_slice(format!("data: {e}\n\n").as_bytes());
    }
    out
}

fn delta(text: &str) -> String {
    serde_json::json!({"choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]})
        .to_string()
}

fn finish(reason: &str) -> String {
    serde_json::json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]}).to_string()
}

const USAGE_CHUNK: &str = r#"{"choices":[],"usage":{"prompt_tokens":120,"completion_tokens":30,"prompt_tokens_details":{"cached_tokens":100},"completion_tokens_details":{"reasoning_tokens":12}}}"#;

fn http_error(status: &str, extra_headers: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn request(reasoning: Effort) -> Request {
    Request {
        instructions: "Translate.".into(),
        input: "[1] 안녕".into(),
        max_output: 4096,
        reasoning,
    }
}

fn model(stub: &Stub) -> ChatCompletions {
    ChatCompletions::new(
        &stub.base_url,
        "test-model",
        Some("sk-test".into()),
        Timeouts {
            connect: Duration::from_secs(5),
            read: Duration::from_secs(5),
        },
    )
}

/// Runs a call and returns its result with everything `on_text` received.
fn run(stub: &Stub, reasoning: Effort) -> (Result<cipher_llm::Completion, LlmError>, Vec<String>) {
    let mut pieces = Vec::new();
    let result = model(stub).complete(&request(reasoning), &mut |t| pieces.push(t.to_owned()));
    (result, pieces)
}

#[test]
fn streams_text_and_reads_usage() {
    let stub = Stub::start(vec![sse(&[
        &delta(""), // role-only first chunk
        &delta("[1] Hel"),
        &delta("lo"),
        &finish("stop"),
        USAGE_CHUNK,
        "[DONE]",
    ])]);
    let (result, pieces) = run(&stub, Effort::ProviderDefault);
    let c = result.unwrap();
    assert_eq!(c.text, "[1] Hello");
    assert!(c.finished);
    assert_eq!(pieces, ["[1] Hel", "lo"]);
    assert_eq!(
        c.usage,
        Usage {
            input: 120,
            output: 30,
            reasoning: Some(12),
            cached: Some(100)
        }
    );
}

#[test]
fn usage_details_are_optional() {
    let stub = Stub::start(vec![sse(&[
        &delta("x"),
        &finish("stop"),
        r#"{"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":1}}"#,
        "[DONE]",
    ])]);
    let c = run(&stub, Effort::ProviderDefault).0.unwrap();
    assert_eq!(
        c.usage,
        Usage {
            input: 5,
            output: 1,
            reasoning: None,
            cached: None
        }
    );
}

#[test]
fn handles_keep_alives_crlf_and_multiline_events() {
    // One event split over two `data:` lines is a single JSON document.
    let json = delta("ok");
    let (a, b) = json.split_at(10);
    let mut body =
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n".to_vec();
    body.extend_from_slice(b": keep-alive\r\n\r\n");
    body.extend_from_slice(format!("data: {a}\r\ndata: {b}\r\n\r\n").as_bytes());
    body.extend_from_slice(b": ping\n\ndata: [DONE]\n\n");
    let stub = Stub::start(vec![body]);
    let c = run(&stub, Effort::ProviderDefault).0.unwrap();
    // JSON allows a newline between tokens, so the joined event parses.
    assert_eq!(c.text, "ok");
    assert!(c.finished, "[DONE] counts as finished");
}

#[test]
fn finish_reason_length_is_not_finished() {
    let stub = Stub::start(vec![sse(&[
        &delta("partial"),
        &finish("length"),
        USAGE_CHUNK,
        "[DONE]",
    ])]);
    let c = run(&stub, Effort::ProviderDefault).0.unwrap();
    assert_eq!(c.text, "partial");
    assert!(!c.finished);
}

#[test]
fn stream_ending_without_finish_is_not_finished() {
    let stub = Stub::start(vec![sse(&[&delta("partial")])]);
    let c = run(&stub, Effort::ProviderDefault).0.unwrap();
    assert_eq!(c.text, "partial");
    assert!(!c.finished);
}

#[test]
fn request_body_and_headers() {
    let stub = Stub::start(vec![sse(&[&finish("stop"), "[DONE]"])]);
    run(&stub, Effort::ProviderDefault).0.unwrap();
    let r = stub.last_request();
    let b = &r.body;
    assert_eq!(b["model"], "test-model");
    assert_eq!(b["stream"], true);
    assert_eq!(b["stream_options"]["include_usage"], true);
    assert_eq!(b["max_completion_tokens"], 4096);
    assert!(b.get("max_tokens").is_none());
    assert!(b.get("reasoning_effort").is_none());
    assert_eq!(b["messages"][0]["role"], "system");
    assert_eq!(b["messages"][0]["content"], "Translate.");
    assert_eq!(b["messages"][1]["role"], "user");
    assert_eq!(b["messages"][1]["content"], "[1] 안녕");
    let auth = r.headers.iter().find(|(k, _)| k == "authorization");
    assert_eq!(auth.map(|(_, v)| v.as_str()), Some("Bearer sk-test"));
}

#[test]
fn reasoning_effort_is_sent_when_set() {
    for (effort, wire) in [
        (Effort::Minimal, "minimal"),
        (Effort::Low, "low"),
        (Effort::Medium, "medium"),
        (Effort::High, "high"),
    ] {
        let stub = Stub::start(vec![sse(&[&finish("stop"), "[DONE]"])]);
        run(&stub, effort).0.unwrap();
        assert_eq!(stub.last_request().body["reasoning_effort"], wire);
    }
}

#[test]
fn no_key_means_no_authorization_header() {
    let stub = Stub::start(vec![sse(&[&finish("stop"), "[DONE]"])]);
    let m = ChatCompletions::new(&stub.base_url, "m", None, Timeouts::default());
    m.complete(&request(Effort::ProviderDefault), &mut |_| {})
        .unwrap();
    assert!(
        stub.last_request()
            .headers
            .iter()
            .all(|(k, _)| k != "authorization")
    );
}

fn error_of(response: Vec<u8>) -> LlmError {
    let stub = Stub::start(vec![response]);
    run(&stub, Effort::ProviderDefault).0.unwrap_err()
}

#[test]
fn http_401_and_403_are_auth() {
    assert_eq!(
        error_of(http_error("401 Unauthorized", "", r#"{"error":"bad key"}"#)),
        LlmError::Auth
    );
    assert_eq!(
        error_of(http_error("403 Forbidden", "", "")),
        LlmError::Auth
    );
}

#[test]
fn http_429_with_retry_after_is_rate_limited() {
    let e = error_of(http_error(
        "429 Too Many Requests",
        "Retry-After: 12\r\n",
        r#"{"error":{"code":"rate_limit_exceeded"}}"#,
    ));
    assert_eq!(
        e,
        LlmError::RateLimited {
            retry_after: Some(Duration::from_secs(12))
        }
    );
}

#[test]
fn http_429_without_retry_after_has_none() {
    let e = error_of(http_error("429 Too Many Requests", "", "{}"));
    assert_eq!(e, LlmError::RateLimited { retry_after: None });
}

#[test]
fn http_429_insufficient_quota_is_usage_limit() {
    let e = error_of(http_error(
        "429 Too Many Requests",
        "",
        r#"{"error":{"type":"insufficient_quota","message":"You exceeded your current quota"}}"#,
    ));
    assert_eq!(e, LlmError::UsageLimit { resets_at: None });
}

#[test]
fn http_500_is_server() {
    let e = error_of(http_error("500 Internal Server Error", "", "boom"));
    assert!(matches!(e, LlmError::Server(m) if m.contains("500") && m.contains("boom")));
}

#[test]
fn http_400_is_rejected_with_body_excerpt() {
    let e = error_of(http_error(
        "400 Bad Request",
        "",
        r#"{"error":{"message":"context length exceeded"}}"#,
    ));
    assert!(matches!(e, LlmError::Rejected(m) if m.contains("context length exceeded")));
}

#[test]
fn error_event_inside_stream_is_server_error_after_partial_text() {
    let stub = Stub::start(vec![sse(&[
        &delta("part"),
        r#"{"error":{"message":"upstream died","code":502}}"#,
    ])]);
    let (result, pieces) = run(&stub, Effort::ProviderDefault);
    assert!(matches!(result, Err(LlmError::Server(m)) if m.contains("upstream died")));
    assert_eq!(pieces, ["part"]);
}

#[test]
fn dropped_connection_mid_stream_is_network_with_partial_text_delivered() {
    // Promise far more bytes than are sent, then close.
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 100000\r\n\r\ndata: {}\n\n",
        delta("[1] Half a sentence")
    )
    .into_bytes();
    response.extend_from_slice(b"data: {\"choices\":[{\"del"); // cut mid-event
    let stub = Stub::start(vec![response]);
    let (result, pieces) = run(&stub, Effort::ProviderDefault);
    assert!(matches!(result, Err(LlmError::Network(_))), "{result:?}");
    assert_eq!(pieces, ["[1] Half a sentence"]);
}

#[test]
fn connection_refused_is_network() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let m = ChatCompletions::new(
        &format!("http://127.0.0.1:{port}/v1"),
        "m",
        None,
        Timeouts::default(),
    );
    let e = m
        .complete(&request(Effort::ProviderDefault), &mut |_| {})
        .unwrap_err();
    assert!(matches!(e, LlmError::Network(_)), "{e:?}");
}

fn endpoint(stub_url: &str, protocol: Protocol, small: Option<&str>) -> Endpoint {
    Endpoint {
        id: "e1".into(),
        name: "Test".into(),
        protocol,
        base_url: stub_url.into(),
        translate_model: "big".into(),
        small_model: small.map(str::to_owned),
        reasoning: Effort::ProviderDefault,
        models: Vec::new(),
    }
}

#[test]
fn connect_picks_models_and_reads_key_from_secrets() {
    for (small, expected_small) in [(Some("tiny"), "tiny"), (None, "big")] {
        let stub = Stub::start(vec![
            sse(&[&finish("stop"), "[DONE]"]),
            sse(&[&finish("stop"), "[DONE]"]),
        ]);
        let ep = endpoint(&stub.base_url, Protocol::ChatCompletions, small);
        let secrets = Arc::new(InMemory::new());
        secrets.set(&ep.api_key_id(), "sk-from-secrets").unwrap();
        let c = connect(&ep, secrets).unwrap();
        for (model, expected) in [(&c.translate, "big"), (&c.small, expected_small)] {
            model
                .complete(&request(Effort::ProviderDefault), &mut |_| {})
                .unwrap();
            let r = stub.last_request();
            assert_eq!(r.body["model"], expected);
            let auth = r.headers.iter().find(|(k, _)| k == "authorization");
            assert_eq!(
                auth.map(|(_, v)| v.as_str()),
                Some("Bearer sk-from-secrets")
            );
        }
    }
}

#[test]
fn chatgpt_needs_a_sign_in_first() {
    let ep = endpoint("", Protocol::ChatGpt, None);
    assert!(matches!(
        connect(&ep, Arc::new(InMemory::new())),
        Err(LlmError::Auth)
    ));
}

#[test]
fn endpoint_serde_round_trip_holds_no_secret() {
    let ep = endpoint("http://x/v1", Protocol::ChatCompletions, Some("tiny"));
    let json = serde_json::to_string(&ep).unwrap();
    assert!(json.contains(r#""protocol":"chat_completions""#));
    assert!(!json.contains("key"));
    assert_eq!(serde_json::from_str::<Endpoint>(&json).unwrap(), ep);
    // Optional fields may be omitted; `off` is accepted for the lowest effort.
    let minimal = r#"{"id":"a","name":"A","protocol":"chat_gpt","base_url":"u","translate_model":"m","reasoning":"off"}"#;
    let ep: Endpoint = serde_json::from_str(minimal).unwrap();
    assert_eq!(ep.small_model, None);
    assert_eq!(ep.reasoning, Effort::Minimal);
}
