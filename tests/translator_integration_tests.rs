use cipher::book::OutputConfig;
use cipher::glossary::GlossaryTerm;
use cipher::translate::{ProviderTextResult, Translator};

mod helpers;

#[tokio::test]
async fn translator_with_mock_provider_returns_configured_translation() {
    let expected = ProviderTextResult {
        chapter: cipher::book::StructuredChapter {
            chapter_number: Some("1".into()),
            chapter_title: Some("Mocked".into()),
            content: "mocked translation".into(),
        },
        usage: Default::default(),
    };

    let provider = helpers::MockProvider::new().with_translation(expected.clone());
    let translator = Translator::new(Box::new(provider));

    let result = translator
        .translate_chapter("raw text", &[], None, OutputConfig::default())
        .await
        .unwrap();

    assert_eq!(result.chapter.content, "mocked translation");
    assert_eq!(result.chapter.chapter_number, Some("1".into()));
}

#[tokio::test]
async fn translator_propagates_translation_error() {
    let provider =
        helpers::MockProvider::new().with_translation_error(anyhow::anyhow!("API error"));
    let translator = Translator::new(Box::new(provider));

    let result = translator
        .translate_chapter("raw text", &[], None, OutputConfig::default())
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("API error"));
}

#[tokio::test]
async fn translator_repair_returns_configured_result() {
    let expected = ProviderTextResult {
        chapter: cipher::book::StructuredChapter {
            chapter_number: Some("1".into()),
            chapter_title: Some("Repaired".into()),
            content: "repaired content".into(),
        },
        usage: Default::default(),
    };

    let provider = helpers::MockProvider::new().with_repair(expected);
    let translator = Translator::new(Box::new(provider));

    let result = translator
        .repair_chapter(
            "raw text",
            "failed translation".into(),
            &[],
            None,
            vec!["missing heading".into()],
            OutputConfig::default(),
        )
        .await
        .unwrap();

    assert_eq!(result.chapter.content, "repaired content");
}

#[tokio::test]
async fn translator_extract_glossary_returns_configured_terms() {
    let terms = vec![GlossaryTerm {
        term: "Term".into(),
        og_term: Some("原词".into()),
        definition: "A test term".into(),
        notes: None,
    }];

    let provider =
        helpers::MockProvider::new().with_extraction(cipher::translate::ProviderGlossaryResult {
            new_glossary_terms: terms.clone(),
            usage: Default::default(),
        });
    let translator = Translator::new(Box::new(provider));

    let result = translator
        .extract_glossary("raw text", "translated text".into(), &[])
        .await
        .unwrap();

    assert_eq!(result.new_glossary_terms.len(), 1);
    assert_eq!(result.new_glossary_terms[0].term, "Term");
}
