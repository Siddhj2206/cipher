use cipher::book::StructuredChapter;
use cipher::translate::providers::Provider;
use cipher::translate::{
    GlossaryExtractionRequest, ProviderGlossaryResult, ProviderTextResult, RepairRequest,
    TranslationRequest,
};

pub struct MockProvider {
    translate_result: Option<anyhow::Result<ProviderTextResult>>,
    repair_result: Option<anyhow::Result<ProviderTextResult>>,
    extract_result: Option<anyhow::Result<ProviderGlossaryResult>>,
}

impl MockProvider {
    pub fn new() -> Self {
        Self {
            translate_result: None,
            repair_result: None,
            extract_result: None,
        }
    }

    pub fn with_translation(mut self, result: ProviderTextResult) -> Self {
        self.translate_result = Some(Ok(result));
        self
    }

    pub fn with_translation_error(mut self, err: anyhow::Error) -> Self {
        self.translate_result = Some(Err(err));
        self
    }

    pub fn with_repair(mut self, result: ProviderTextResult) -> Self {
        self.repair_result = Some(Ok(result));
        self
    }

    pub fn with_extraction(mut self, result: ProviderGlossaryResult) -> Self {
        self.extract_result = Some(Ok(result));
        self
    }
}

fn default_text_result() -> ProviderTextResult {
    ProviderTextResult {
        chapter: StructuredChapter {
            chapter_number: Some("1".into()),
            chapter_title: Some("Test".into()),
            content: "translated content".into(),
        },
        usage: Default::default(),
    }
}

fn default_extract_result() -> ProviderGlossaryResult {
    ProviderGlossaryResult {
        new_glossary_terms: vec![],
        usage: Default::default(),
    }
}

#[async_trait::async_trait]
impl Provider for MockProvider {
    async fn translate(&self, _req: TranslationRequest) -> anyhow::Result<ProviderTextResult> {
        match &self.translate_result {
            Some(result) => match result {
                Ok(v) => Ok(v.clone()),
                Err(e) => Err(anyhow::anyhow!("{}", e)),
            },
            None => Ok(default_text_result()),
        }
    }

    async fn repair(&self, _req: RepairRequest) -> anyhow::Result<ProviderTextResult> {
        match &self.repair_result {
            Some(result) => match result {
                Ok(v) => Ok(v.clone()),
                Err(e) => Err(anyhow::anyhow!("{}", e)),
            },
            None => Ok(default_text_result()),
        }
    }

    async fn extract_glossary(
        &self,
        _req: GlossaryExtractionRequest,
    ) -> anyhow::Result<ProviderGlossaryResult> {
        match &self.extract_result {
            Some(result) => match result {
                Ok(v) => Ok(v.clone()),
                Err(e) => Err(anyhow::anyhow!("{}", e)),
            },
            None => Ok(default_extract_result()),
        }
    }
}
