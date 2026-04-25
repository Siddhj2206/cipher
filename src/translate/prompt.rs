//! Prompt building for translation requests
//!
//! Uses the base prompt and format from Book-Translator-Go

use crate::book::OutputConfig;
use crate::glossary::GlossaryTerm;
use crate::translate::{GlossaryExtractionRequest, RepairRequest, TranslationRequest};

const BASE_PROMPT: &str = r#"You are an expert translator working on a serialized web novel. Your task is to translate a chapter from its original language (likely Korean or Chinese) into high-quality English prose.

**Tone and Style Requirements:**

1.  **Atmosphere:** Accurately capture the tone and atmosphere of the original text. If the source is serious and dramatic, the translation must be serious and dramatic. If it's lighthearted, keep it lighthearted.
2.  **Dialogue:** Dialogue must be natural, dynamic, and appropriate for each character's personality and social standing. Avoid stiff, robotic, or overly literal translations. Characters should sound like real people.
3.  **Pacing and Flow:** The narrative should flow smoothly. Pay attention to the rhythm of sentences and paragraphs. Do not break the pacing with awkward phrasing.
4.  **Nuance:** Be sensitive to cultural nuances. If a direct translation doesn't convey the meaning in English, adapt it to a functional equivalent that captures the intent while maintaining the story's context.
5.  **Readability:** The final text must be immersive and enjoyable for a native English speaker. Avoid transliteration (e.g., writing Korean words in English letters) unless it is a specific name or a sound effect.

**Primary Goal:**
Produce a translation that is both accurate to the source and captivating to read, ensuring the reader experiences the story as the author intended."#;

const FORMATTING_REQUIREMENTS: &str = r#"**Formatting Requirements: [IMPORTANT]**

  * The translated prose must remain proper Markdown.
  * Preserve the original paragraph structure and line breaks. Do **not** merge paragraphs into a single block of text.
  * Maintain proper spacing between paragraphs.
  * Keep dialogue formatting intact (e.g., use of quotation marks and new lines for each speaker)."#;

fn build_style_section(style_guide: &Option<String>) -> String {
    match style_guide {
        Some(guide) if !guide.trim().is_empty() => {
            format!(
                r#"

**Style Guide:**

Follow these additional style and tone instructions carefully:

{}
"#,
                guide.trim()
            )
        }
        _ => String::new(),
    }
}

/// Build the full translation prompt for a chapter
pub fn build_translation_prompt(req: &TranslationRequest) -> String {
    let glossary_section = build_glossary_section(&req.glossary_terms);
    let style_section = build_style_section(&req.style_guide);
    let output_section = build_output_section(&req.output_config);
    format!(
        r#"**Project Overview & Core Task:**

{}
{}
{}
**Glossary Requirements:**

Adhere strictly to the established glossary below for consistency.

**Established Glossary:**
{}

Do not return glossary metadata or new glossary terms in this response. Glossary extraction is handled separately after the translation is accepted.

**Text to Translate:**
{}

Return your response as a JSON object with exactly these fields:
- "chapter_number": string or null
- "chapter_title": string or null
- "content": string containing the translated markdown body without the top heading"#,
        BASE_PROMPT, style_section, output_section, glossary_section, req.chapter_markdown
    )
}

pub fn build_repair_prompt(
    req: &RepairRequest,
    glossary_section: &str,
    style_section: &str,
) -> String {
    let errors_list = req
        .validation_errors
        .iter()
        .map(|e| format!("- {}", e))
        .collect::<Vec<_>>()
        .join("\n");
    let output_section = build_output_section(&req.output_config);

    format!(
        r#"**Project Overview & Core Task:**

{}
{}
{}
**Glossary Requirements:**

Adhere strictly to the established glossary below for consistency.

**Established Glossary:**
{}

**REPAIR REQUEST:**

Your previous translation had the following validation errors:
{}

**Original text to translate:**
{}

**Your previous (failed) translation:**
{}

Please fix the issues above and provide a corrected translation. Do not invent glossary entries or return any glossary metadata. Pay special attention to the validation errors listed.

{}

Return your response as a JSON object with exactly these fields:
- "chapter_number": string or null
- "chapter_title": string or null
- "content": string containing the translated markdown body without the top heading"#,
        BASE_PROMPT,
        style_section,
        output_section,
        glossary_section,
        errors_list,
        req.chapter_markdown,
        req.failed_translation,
        FORMATTING_REQUIREMENTS
    )
}

fn build_output_section(config: &OutputConfig) -> String {
    let chapter_number_desc = config
        .fields
        .chapter_number
        .description
        .as_deref()
        .unwrap_or("Chapter number when one is present");
    let chapter_title_desc = config
        .fields
        .chapter_title
        .description
        .as_deref()
        .unwrap_or("Chapter title when one is present");
    let content_desc = config
        .fields
        .content
        .description
        .as_deref()
        .unwrap_or("Main translated chapter body in markdown, excluding the top heading");

    format!(
        r#"**Structured Output Requirements: [IMPORTANT]**

Return semantic chapter fields, not final rendered markdown.

- `chapter_number`: {}{}
- `chapter_title`: {}{}
- `content`: {}{}

The final markdown will be rendered locally using this template:

```text
{}
```

`content` must contain only the main chapter body. Do not include the top heading inside `content`."#,
        chapter_number_desc,
        required_suffix(config.fields.chapter_number.required),
        chapter_title_desc,
        required_suffix(config.fields.chapter_title.required),
        content_desc,
        required_suffix(config.fields.content.required),
        config.render.template
    )
}

fn required_suffix(required: bool) -> &'static str {
    if required {
        " (required)"
    } else {
        " (optional)"
    }
}

pub fn build_glossary_extraction_prompt(req: &GlossaryExtractionRequest) -> String {
    let glossary_section = build_existing_glossary_keys_section(&req.existing_glossary_terms);

    format!(
        r#"You are reviewing an accepted chapter translation for glossary extraction.

Identify only *new*, absolutely essential glossary terms that should be added for future chapters. Be **extremely** selective. A term should only be added if it meets **all** of the following criteria:

1.  Has a specific non-English name requiring consistent translation.
2.  Will definitely appear again (main characters/major locations only).
3.  Would cause significant reader confusion if translated inconsistently.

When in doubt, **do not** add the term. Format every extracted term with "term", "og_term", and "definition" fields. The "og_term" field should contain the original language term when known.

**Existing glossary terms:**
{}

Do not return terms that are already present in the existing glossary. If a term appears in the existing glossary, omit it even if it appears in this chapter.

**Original source chapter:**
{}

**Accepted translation:**
{}

Return your response as a JSON object with exactly one field:
- "new_glossary_terms": array of glossary term objects"#,
        glossary_section, req.chapter_markdown, req.translated_markdown
    )
}

fn build_existing_glossary_keys_section(terms: &[GlossaryTerm]) -> String {
    if terms.is_empty() {
        "(No existing glossary terms)".to_string()
    } else {
        terms
            .iter()
            .map(|t| match t.og_term.as_deref() {
                Some(og) => format!("{} [{}]", t.term, og),
                None => t.term.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn build_glossary_section(terms: &[GlossaryTerm]) -> String {
    if terms.is_empty() {
        "(No glossary terms available)".to_string()
    } else {
        terms
            .iter()
            .map(|t| {
                if let Some(ref og) = t.og_term {
                    format!("{} [{}]: {}", t.term, og, t.definition)
                } else {
                    format!("{}: {}", t.term, t.definition)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glossary::GlossaryTerm;

    #[test]
    fn test_build_translation_prompt_includes_base() {
        let req = translation_request("# Chapter 1\n\nHello", Vec::new());
        let prompt = build_translation_prompt(&req);
        assert!(prompt.contains("expert translator"));
        assert!(prompt.contains("Chapter 1"));
        assert!(prompt.contains("chapter_number"));
        assert!(!prompt.contains("new_glossary_terms"));
        assert!(!prompt.contains("Following the translation"));
        assert!(prompt.contains("Glossary extraction is handled separately"));
    }

    #[test]
    fn test_build_translation_prompt_with_glossary() {
        let terms = vec![GlossaryTerm {
            term: "Magic".to_string(),
            og_term: Some("마법".to_string()),
            definition: "Supernatural power".to_string(),
            notes: None,
        }];
        let req = translation_request("Text", terms);
        let prompt = build_translation_prompt(&req);
        assert!(prompt.contains("Magic [마법]: Supernatural power"));
    }

    #[test]
    fn test_build_translation_prompt_without_glossary() {
        let req = translation_request("Text", Vec::new());
        let prompt = build_translation_prompt(&req);
        assert!(prompt.contains("(No glossary terms available)"));
    }

    #[test]
    fn test_build_repair_prompt_includes_errors() {
        let req = RepairRequest {
            chapter_markdown: "Original text".to_string(),
            glossary_terms: Vec::new(),
            style_guide: None,
            failed_translation: "Bad translation".to_string(),
            validation_errors: vec![
                "Missing chapter heading".to_string(),
                "Unbalanced code fences".to_string(),
            ],
            output_config: OutputConfig::default(),
        };
        let prompt = build_repair_prompt(&req, "(No glossary terms available)", "");

        assert!(prompt.contains("REPAIR REQUEST"));
        assert!(prompt.contains("Missing chapter heading"));
        assert!(prompt.contains("Unbalanced code fences"));
        assert!(prompt.contains("Original text"));
        assert!(prompt.contains("Bad translation"));
        assert!(!prompt.contains("new_glossary_terms"));
    }

    #[test]
    fn test_build_glossary_extraction_prompt_requests_terms_only() {
        let req = GlossaryExtractionRequest {
            chapter_markdown: "Source".to_string(),
            translated_markdown: "# Chapter 1\n\nTranslated".to_string(),
            existing_glossary_terms: vec![GlossaryTerm {
                term: "Magic".to_string(),
                og_term: Some("마법".to_string()),
                definition: "Supernatural power".to_string(),
                notes: None,
            }],
        };
        let prompt = build_glossary_extraction_prompt(&req);
        assert!(prompt.contains("Accepted translation"));
        assert!(prompt.contains("Magic [마법]"));
        assert!(prompt.contains("Do not return terms that are already present"));
        assert!(prompt.contains("new_glossary_terms"));
        assert!(!prompt.contains("\"translation\":"));
    }

    fn translation_request(
        chapter_markdown: &str,
        glossary_terms: Vec<GlossaryTerm>,
    ) -> TranslationRequest {
        TranslationRequest {
            chapter_markdown: chapter_markdown.to_string(),
            glossary_terms,
            style_guide: None,
            output_config: OutputConfig::default(),
        }
    }
}
