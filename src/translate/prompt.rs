//! Prompt building for translation requests
//!
//! Uses the base prompt and format from Book-Translator-Go

use crate::glossary::GlossaryTerm;
use crate::translate::TranslationRequest;

const BASE_PROMPT: &str = r#"You are an expert translator working on a serialized web novel. Your task is to translate a chapter from its original language (likely Korean or Chinese) into high-quality English prose.

**Tone and Style Requirements:**

1.  **Atmosphere:** Accurately capture the tone and atmosphere of the original text. If the source is serious and dramatic, the translation must be serious and dramatic. If it's lighthearted, keep it lighthearted.
2.  **Dialogue:** Dialogue must be natural, dynamic, and appropriate for each character's personality and social standing. Avoid stiff, robotic, or overly literal translations. Characters should sound like real people.
3.  **Pacing and Flow:** The narrative should flow smoothly. Pay attention to the rhythm of sentences and paragraphs. Do not break the pacing with awkward phrasing.
4.  **Nuance:** Be sensitive to cultural nuances. If a direct translation doesn't convey the meaning in English, adapt it to a functional equivalent that captures the intent while maintaining the story's context.
5.  **Readability:** The final text must be immersive and enjoyable for a native English speaker. Avoid transliteration (e.g., writing Korean words in English letters) unless it is a specific name or a sound effect.

**Primary Goal:**
Produce a translation that is both accurate to the source and captivating to read, ensuring the reader experiences the story as the author intended."#;

/// Build the full translation prompt for a chapter
pub fn build_prompt(req: &TranslationRequest) -> String {
    let glossary_section = build_glossary_section(&req.glossary_terms);

    if req.is_repair() {
        build_repair_prompt(req, &glossary_section)
    } else {
        build_initial_prompt(req, &glossary_section)
    }
}

fn build_initial_prompt(req: &TranslationRequest, glossary_section: &str) -> String {
    format!(
        r#"**Project Overview & Core Task:**

{}

**Glossary and New Terms:**

Adhere strictly to the established glossary below for consistency.

**Established Glossary:**
{}

Following the translation, you are to identify any *new*, absolutely essential terms that must be added to the glossary for future chapters. Be **extremely** selective. A term should only be added if it meets **all** of the following criteria:

1.  Has a specific non-English name requiring consistent translation.
2.  Will definitely appear again (main characters/major locations only).
3.  Would cause significant reader confusion if translated inconsistently.

When in doubt, **do not** add the term. Format new terms as an array of objects with "term", "og_term", and "definition" fields. The "og_term" field should contain the original language term (e.g., Korean characters), while "term" contains the English name.

**Formatting Requirements: [IMPORTANT]**

  * The final output must be in proper Markdown.
  * Start with a top-level heading ('#') for the chapter.
      * If the original has a chapter number and title like 'X: [Chapter Title]', format it as: '# Chapter X: [Chapter Title]'. Even if the original may not have '# Chapter', use it in the translation
      * If only a number is present, use: '# Chapter X'
  * Preserve the original paragraph structure and line breaks. Do **not** merge paragraphs into a single block of text.
  * Maintain proper spacing between paragraphs.
  * Keep dialogue formatting intact (e.g., use of quotation marks and new lines for each speaker).

**Text to Translate:**
{}

Return your response as a JSON object with exactly two fields:
- "translation": string containing the translated markdown
- "new_glossary_terms": array of glossary term objects"#,
        BASE_PROMPT, glossary_section, req.chapter_markdown
    )
}

fn build_repair_prompt(req: &TranslationRequest, glossary_section: &str) -> String {
    let errors_list = req
        .validation_errors
        .iter()
        .map(|e| format!("- {}", e))
        .collect::<Vec<_>>()
        .join("\n");

    let failed = req.failed_translation.as_deref().unwrap_or("");

    format!(
        r#"**Project Overview & Core Task:**

{}

**Glossary and New Terms:**

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

Please fix the issues above and provide a corrected translation. Pay special attention to the validation errors listed.

**Formatting Requirements: [IMPORTANT]**

  * The final output must be in proper Markdown.
  * Start with a top-level heading ('#') for the chapter.
      * If the original has a chapter number and title like 'X: [Chapter Title]', format it as: '# Chapter X: [Chapter Title]'. Even if the original may not have '# Chapter', use it in the translation
      * If only a number is present, use: '# Chapter X'
  * Preserve the original paragraph structure and line breaks. Do **not** merge paragraphs into a single block of text.
  * Maintain proper spacing between paragraphs.
  * Keep dialogue formatting intact (e.g., use of quotation marks and new lines for each speaker).

Return your response as a JSON object with exactly two fields:
- "translation": string containing the translated markdown
- "new_glossary_terms": array of glossary term objects"#,
        BASE_PROMPT, glossary_section, errors_list, req.chapter_markdown, failed
    )
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
    fn test_build_prompt_includes_base() {
        let req = TranslationRequest::new("# Chapter 1\n\nHello".to_string());
        let prompt = build_prompt(&req);
        assert!(prompt.contains("expert translator"));
        assert!(prompt.contains("Chapter 1"));
    }

    #[test]
    fn test_build_prompt_with_glossary() {
        let terms = vec![GlossaryTerm {
            term: "Magic".to_string(),
            og_term: Some("마법".to_string()),
            definition: "Supernatural power".to_string(),
            notes: None,
        }];
        let req = TranslationRequest::new("Text".to_string()).with_glossary_terms(terms);
        let prompt = build_prompt(&req);
        assert!(prompt.contains("Magic [마법]: Supernatural power"));
    }

    #[test]
    fn test_build_prompt_without_glossary() {
        let req = TranslationRequest::new("Text".to_string());
        let prompt = build_prompt(&req);
        assert!(prompt.contains("(No glossary terms available)"));
    }

    #[test]
    fn test_build_repair_prompt_includes_errors() {
        let req = TranslationRequest::new("Original text".to_string())
            .with_failed_translation("Bad translation".to_string())
            .with_validation_errors(vec![
                "Missing chapter heading".to_string(),
                "Unbalanced code fences".to_string(),
            ]);
        let prompt = build_prompt(&req);

        assert!(prompt.contains("REPAIR REQUEST"));
        assert!(prompt.contains("Missing chapter heading"));
        assert!(prompt.contains("Unbalanced code fences"));
        assert!(prompt.contains("Original text"));
        assert!(prompt.contains("Bad translation"));
    }

    #[test]
    fn test_build_repair_prompt_is_repair() {
        let req = TranslationRequest::new("Text".to_string())
            .with_failed_translation("Failed".to_string())
            .with_validation_errors(vec!["Error".to_string()]);
        assert!(req.is_repair());

        let normal_req = TranslationRequest::new("Text".to_string());
        assert!(!normal_req.is_repair());
    }
}
