use cipher_llm::{Effort, Fake, FakeReply, LlmError, Model, Request, Usage};

fn req(input: &str) -> Request {
    Request {
        instructions: "sys".into(),
        input: input.into(),
        max_output: 100,
        reasoning: Effort::Low,
    }
}

#[test]
fn replays_script_in_order_and_records_requests() {
    let fake = Fake::new([
        FakeReply::Text("[1] First reply, long enough to be chunked".into()),
        FakeReply::CutOff("[1] Cut".into()),
        FakeReply::Fail(LlmError::Auth),
    ]);
    let model: Box<dyn Model> = Box::new(fake.clone());

    let mut pieces = Vec::new();
    let c = model
        .complete(&req("a"), &mut |t| pieces.push(t.to_owned()))
        .unwrap();
    assert!(c.finished);
    assert_eq!(c.text, "[1] First reply, long enough to be chunked");
    assert!(pieces.len() > 1, "text arrives in small chunks");
    assert_eq!(pieces.concat(), c.text);
    assert!(c.usage.output > 0 && c.usage.input > 0);

    let c = model.complete(&req("b"), &mut |_| {}).unwrap();
    assert!(!c.finished);
    assert_eq!(c.text, "[1] Cut");

    assert_eq!(
        model.complete(&req("c"), &mut |_| {}).unwrap_err(),
        LlmError::Auth
    );

    let seen = fake.requests();
    assert_eq!(
        seen.iter().map(|r| r.input.as_str()).collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert_eq!(seen[0], req("a"));
    assert_eq!(fake.remaining(), 0);
}

#[test]
fn running_out_of_script_is_a_clear_error() {
    let fake = Fake::new([]);
    let err = fake.complete(&req("x"), &mut |_| {}).unwrap_err();
    assert!(matches!(err, LlmError::Rejected(m) if m.contains("script ran out")));
    assert_eq!(fake.requests().len(), 1, "the request is still recorded");
}

#[test]
fn chunking_respects_multibyte_characters() {
    let fake = Fake::new([FakeReply::Text(
        "안녕하세요, 세계! 번역 테스트입니다.".into(),
    )]);
    let mut pieces = Vec::new();
    let c = fake
        .complete(&req("x"), &mut |t| pieces.push(t.to_owned()))
        .unwrap();
    assert_eq!(pieces.concat(), c.text);
}

#[test]
fn usage_adds_up() {
    let mut total = Usage::default();
    total.add(&Usage {
        input: 1,
        output: 2,
        reasoning: Some(1),
        cached: None,
    });
    total.add(&Usage {
        input: 3,
        output: 4,
        reasoning: None,
        cached: Some(2),
    });
    assert_eq!(
        total,
        Usage {
            input: 4,
            output: 6,
            reasoning: Some(1),
            cached: Some(2)
        }
    );
}
