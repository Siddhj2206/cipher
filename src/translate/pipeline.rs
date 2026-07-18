use crate::book::{OutputConfig, StructuredChapter, render_chapter_markdown, render_starts_with_markdown_heading, validate_structured_chapter};
use crate::glossary::{GlossaryTerm, SelectionResult};
use crate::translate::cmd::Translators;
use crate::translate::{ProviderTranslationResult, TranslationUsage, Translator};
use crate::validate::{ValidationOptions, validate_translation};

use crate::output::verbose_detail_kv;

const MAX_API_RETRIES: usize = 3;

pub(crate) struct ChapterPipeline {
    translators: Translators,
}

pub(crate) struct PipelineResult {
    pub translation_result: Option<ProviderTranslationResult>,
    pub last_error: Option<String>,
    pub failed_usage: Option<TranslationUsage>,
    pub repair_attempted: bool,
}

impl ChapterPipeline {
    pub(crate) fn new(translators: Translators) -> Self {
        Self { translators }
    }

    pub(crate) async fn process(
        &self,
        chapter_text: &str,
        selection: &SelectionResult,
        glossary: &[GlossaryTerm],
        style_guide: &Option<String>,
        output_config: &OutputConfig,
    ) -> PipelineResult {
        let (response, last_error, failed_usage, repair_attempted) = self
            .attempt_translation(chapter_text, selection, glossary, style_guide, output_config)
            .await;

        PipelineResult {
            translation_result: response,
            last_error,
            failed_usage,
            repair_attempted,
        }
    }

    async fn attempt_translation(
        &self,
        chapter_text: &str,
        selection: &SelectionResult,
        glossary: &[GlossaryTerm],
        style_guide: &Option<String>,
        output_config: &OutputConfig,
    ) -> (
        Option<ProviderTranslationResult>,
        Option<String>,
        Option<TranslationUsage>,
        bool,
    ) {
        let mut last_error: Option<String> = None;
        let mut failed_usage: Option<TranslationUsage> = None;
        let mut repair_attempted = false;
        let validation_options = ValidationOptions {
            require_markdown_heading: render_starts_with_markdown_heading(output_config),
        };

        for api_attempt in 1..=MAX_API_RETRIES {
            match self
                .translators
                .translation
                .translate_chapter(
                    chapter_text,
                    &selection.terms,
                    style_guide.clone(),
                    output_config.clone(),
                )
                .await
            {
                Ok(resp) => {
                    let rendered = render_chapter_markdown(&resp.chapter, output_config);
                    let mut validation_errors =
                        validate_structured_chapter(&resp.chapter, output_config);
                    let rendered_validation =
                        validate_translation(&rendered, validation_options);
                    validation_errors.extend(rendered_validation.errors().iter().cloned());
                    let original_translation_usage = resp.usage.clone();
                    failed_usage = Some(original_translation_usage.clone());

                    if validation_errors.is_empty() {
                        let result = finish_accepted_translation(
                            &self.translators.glossary,
                            chapter_text,
                            resp.chapter,
                            rendered,
                            glossary,
                            resp.usage,
                        )
                        .await;
                        return (Some(result), None, None, false);
                    }

                    last_error = Some(format!(
                        "Validation failed: {}",
                        validation_errors.join(", ")
                    ));

                    if api_attempt == 1 {
                        verbose_detail_kv(
                            "Validation",
                            format!("{} Attempting repair.", validation_errors.join(", ")),
                        );

                        repair_attempted = true;

                        match self
                            .translators
                            .repair
                            .repair_chapter(
                                chapter_text,
                                rendered,
                                &selection.terms,
                                style_guide.clone(),
                                validation_errors,
                                output_config.clone(),
                            )
                            .await
                        {
                            Ok(repair_resp) => {
                                let repaired_rendered =
                                    render_chapter_markdown(&repair_resp.chapter, output_config);
                                let mut combined_usage = original_translation_usage.clone();
                                combined_usage += repair_resp.usage.clone();
                                failed_usage = Some(combined_usage.clone());
                                let mut repair_errors =
                                    validate_structured_chapter(&repair_resp.chapter, output_config);
                                let repair_validation =
                                    validate_translation(&repaired_rendered, validation_options);
                                repair_errors.extend(repair_validation.errors().iter().cloned());

                                if repair_errors.is_empty() {
                                    verbose_detail_kv("Repair", "success");
                                    let result = finish_accepted_translation(
                                        &self.translators.glossary,
                                        chapter_text,
                                        repair_resp.chapter,
                                        repaired_rendered,
                                        glossary,
                                        combined_usage,
                                    )
                                    .await;
                                    return (Some(result), None, None, repair_attempted);
                                } else {
                                    last_error = Some(format!(
                                        "Repair failed validation: {}",
                                        repair_errors.join(", ")
                                    ));
                                    verbose_detail_kv("Repair", last_error.as_ref().unwrap());
                                }
                            }
                            Err(e) => {
                                last_error = Some(format!("Repair request failed: {}", e));
                                verbose_detail_kv("Repair", last_error.as_ref().unwrap());
                            }
                        }
                    }

                    break;
                }
                Err(e) => {
                    last_error = Some(format!("API error: {}", e));
                    if api_attempt < MAX_API_RETRIES {
                        let delay_secs = 2u64.pow(api_attempt as u32);
                        verbose_detail_kv(
                            "Attempt",
                            format!(
                                "Attempt {}/{} failed: {}. Retrying in {}s.",
                                api_attempt,
                                MAX_API_RETRIES,
                                last_error.as_ref().unwrap(),
                                delay_secs
                            ),
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                    }
                }
            }
        }

        (None, last_error, failed_usage, repair_attempted)
}
}

pub(crate) async fn finish_accepted_translation(
    translator: &Translator,
    chapter_text: &str,
    chapter: StructuredChapter,
    rendered_markdown: String,
    glossary: &[GlossaryTerm],
    mut usage: TranslationUsage,
) -> ProviderTranslationResult {
    let new_glossary_terms = match translator
        .extract_glossary(chapter_text, rendered_markdown, glossary)
        .await
    {
        Ok(glossary_resp) => {
            usage += glossary_resp.usage;
            verbose_detail_kv("Glossary extraction", "success");
            glossary_resp.new_glossary_terms
        }
        Err(e) => {
            verbose_detail_kv(
                "Glossary extraction",
                format!("failed: {}. Chapter kept without new terms.", e),
            );
            Vec::new()
        }
    };

    ProviderTranslationResult {
        response: crate::translate::AcceptedTranslation {
            chapter,
            new_glossary_terms,
        },
        usage,
    }
}
