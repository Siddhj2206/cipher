#[derive(Debug, Clone, Copy)]
pub struct ValidationOptions {
    pub require_heading: bool,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            require_heading: true,
        }
    }
}

pub fn validate_translation(text: &str, options: ValidationOptions) -> ValidationResult {
    let mut errors = Vec::new();

    if text.trim().is_empty() {
        errors.push("Translation is empty".to_string());
    }

    if options.require_heading {
        let trimmed = text.trim_start();
        if !trimmed.starts_with('#') {
            errors.push("Translation must start with a heading (#)".to_string());
        } else {
            let first_line = trimmed.lines().next().unwrap_or("");
            if !is_valid_chapter_heading(first_line) {
                errors.push(format!(
                    "Chapter heading must start with '# ' and have content, got: {}",
                    first_line
                ));
            }
        }
    }

    if !has_balanced_code_fences(text) {
        errors.push("Unbalanced code fences (```)".to_string());
    }

    check_json_leakage(text, &mut errors);

    if errors.is_empty() {
        ValidationResult::Valid
    } else {
        ValidationResult::Invalid(errors)
    }
}

fn check_json_leakage(text: &str, errors: &mut Vec<String>) {
    let schema_patterns = [
        ("\"type\":", "Schema pattern detected: \"type\":"),
        (
            "\"properties\":",
            "Schema pattern detected: \"properties\":",
        ),
        ("\"$ref\"", "Schema pattern detected: \"$ref\""),
        ("\"required\":", "Schema pattern detected: \"required\":"),
    ];

    let response_patterns = [
        (
            "\"translation\":",
            "Response schema leaked: \"translation\":",
        ),
        (
            "\"new_glossary_terms\":",
            "Response schema leaked: \"new_glossary_terms\":",
        ),
    ];

    for (pattern, msg) in schema_patterns {
        if is_pattern_in_json_context(text, pattern) {
            errors.push(msg.to_string());
        }
    }

    for (pattern, msg) in response_patterns {
        if is_pattern_in_json_context(text, pattern) {
            errors.push(msg.to_string());
        }
    }

    if looks_like_json_object(text) {
        errors.push("Output appears to be raw JSON instead of markdown".to_string());
    }
}

fn is_pattern_in_json_context(text: &str, pattern: &str) -> bool {
    let mut search_start = 0;
    while let Some(offset) = text[search_start..].find(pattern) {
        let pos = search_start + offset;
        if pos == 0 {
            return true;
        }
        let before = &text[..pos];
        let last_char = before.chars().last().unwrap_or(' ');
        if matches!(last_char, '{' | ',' | '\n') {
            return true;
        }
        let before_trimmed = before.trim_end();
        if before_trimmed.is_empty() {
            return true;
        }
        // Also check last non-whitespace char (handles ", \"key\":" patterns)
        let last_trimmed = before_trimmed.chars().last().unwrap_or(' ');
        if matches!(last_trimmed, '{' | ',' | '\n') {
            return true;
        }
        search_start = pos + pattern.len();
    }
    false
}

fn looks_like_json_object(text: &str) -> bool {
    let trimmed = text.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return false;
    }
    let brace_count = trimmed.chars().filter(|&c| c == '{').count();
    brace_count > 0 && trimmed.matches(':').count() >= brace_count
}

fn is_valid_chapter_heading(line: &str) -> bool {
    let line = line.trim();
    if !line.starts_with("# ") {
        return false;
    }
    let rest = &line[2..];
    !rest.trim().is_empty()
}

fn has_balanced_code_fences(text: &str) -> bool {
    let mut fence_stack: Vec<usize> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let backtick_count = trimmed.chars().take_while(|&c| c == '`').count();
        if backtick_count >= 3 {
            if let Some(&open_len) = fence_stack.last() {
                if backtick_count >= open_len {
                    fence_stack.pop();
                } else {
                    fence_stack.push(backtick_count);
                }
            } else {
                fence_stack.push(backtick_count);
            }
        }
    }
    fence_stack.is_empty()
}

#[derive(Debug, Clone)]
pub enum ValidationResult {
    Valid,
    Invalid(Vec<String>),
}

impl ValidationResult {
    #[cfg(test)]
    pub fn is_valid(&self) -> bool {
        matches!(self, ValidationResult::Valid)
    }

    pub fn errors(&self) -> &[String] {
        match self {
            ValidationResult::Valid => &[],
            ValidationResult::Invalid(errors) => errors,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_chapter_headings() {
        assert!(is_valid_chapter_heading("# Chapter 1"));
        assert!(is_valid_chapter_heading("# Chapter 1: The Beginning"));
        assert!(is_valid_chapter_heading("# Prologue"));
        assert!(is_valid_chapter_heading("# Epilogue"));
        assert!(is_valid_chapter_heading("# Chapter One"));
        assert!(is_valid_chapter_heading("# Title Here"));
    }

    #[test]
    fn test_invalid_chapter_headings() {
        assert!(!is_valid_chapter_heading("#"));
        assert!(!is_valid_chapter_heading("# "));
        assert!(!is_valid_chapter_heading("Some text"));
        assert!(!is_valid_chapter_heading("## Heading 2"));
    }

    #[test]
    fn test_balanced_fences() {
        assert!(has_balanced_code_fences("```rust\ncode\n```"));
        assert!(has_balanced_code_fences("No code fences"));
        assert!(!has_balanced_code_fences("```rust\ncode"));
        assert!(has_balanced_code_fences("```\n```\n```\n```"));
    }

    #[test]
    fn test_json_leakage_detection() {
        let result =
            validate_translation("{\"translation\": \"text\"}", ValidationOptions::default());
        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.contains("raw JSON")));

        let result = validate_translation(
            "# Prologue\n\n{\n\"type\": \"string\"\n}",
            ValidationOptions::default(),
        );
        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.contains("Schema pattern")));

        let result = validate_translation(
            "# Chapter\n\nThe character said \"type\": in dialogue.",
            ValidationOptions::default(),
        );
        assert!(result.is_valid());

        let result = validate_translation(
            "# Chapter 1\n\n\"new_glossary_terms\": []",
            ValidationOptions::default(),
        );
        assert!(!result.is_valid());
        assert!(
            result
                .errors()
                .iter()
                .any(|e| e.contains("Response schema leaked"))
        );

        let result = validate_translation(
            "# Prologue\n\nNormal text without JSON.",
            ValidationOptions::default(),
        );
        assert!(result.is_valid());
    }

    #[test]
    fn test_valid_translation_passes() {
        let text = "# Chapter 1\n\nThis is a valid translation.";
        assert!(validate_translation(text, ValidationOptions::default()).is_valid());
    }

    #[test]
    fn test_prologue_epilogue_passes() {
        assert!(
            validate_translation("# Prologue\n\nSome text", ValidationOptions::default())
                .is_valid()
        );
        assert!(
            validate_translation("# Epilogue\n\nSome text", ValidationOptions::default())
                .is_valid()
        );
    }

    #[test]
    fn test_is_pattern_in_json_context_at_start() {
        assert!(is_pattern_in_json_context("\"type\":", "\"type\":"));
    }

    #[test]
    fn test_is_pattern_in_json_context_after_brace() {
        assert!(is_pattern_in_json_context(
            "{\"type\": \"string\"}",
            "\"type\":"
        ));
    }

    #[test]
    fn test_is_pattern_in_json_context_after_comma() {
        assert!(is_pattern_in_json_context(
            "{\"a\": 1, \"type\": \"string\"}",
            "\"type\":"
        ));
    }

    #[test]
    fn test_is_pattern_in_json_context_after_newline() {
        assert!(is_pattern_in_json_context(
            "{\n\"type\": \"string\"}",
            "\"type\":"
        ));
    }

    #[test]
    fn test_is_pattern_in_json_context_in_prose_not_detected() {
        // Pattern appears in normal prose context (after a letter)
        assert!(!is_pattern_in_json_context(
            "The character said \"type\": something",
            "\"type\":"
        ));
    }

    #[test]
    fn test_is_pattern_in_json_context_checks_all_occurrences() {
        // First occurrence is in prose, second is in JSON context
        assert!(is_pattern_in_json_context(
            "He said \"type\": foo, then\n\"type\": \"string\"",
            "\"type\":"
        ));
    }

    #[test]
    fn test_is_pattern_not_found() {
        assert!(!is_pattern_in_json_context("no match here", "\"type\":"));
    }

    #[test]
    fn test_looks_like_json_object_valid() {
        assert!(looks_like_json_object("{\"key\": \"value\"}"));
        assert!(looks_like_json_object(
            "  { \"a\": 1, \"b\": { \"c\": 2 } }  "
        ));
    }

    #[test]
    fn test_looks_like_json_object_invalid() {
        assert!(!looks_like_json_object("not json"));
        assert!(!looks_like_json_object("{no colon here}"));
        assert!(!looks_like_json_object("[\"array\"]"));
    }

    #[test]
    fn test_empty_translation() {
        let result = validate_translation("", ValidationOptions::default());
        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.contains("empty")));
    }

    #[test]
    fn test_missing_heading() {
        let result = validate_translation(
            "Just some text without heading.",
            ValidationOptions::default(),
        );
        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.contains("heading")));
    }

    #[test]
    fn test_unbalanced_code_fences() {
        let result = validate_translation(
            "# Chapter 1\n\n```rust\nlet x = 1;",
            ValidationOptions::default(),
        );
        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.contains("code fences")));
    }

    #[test]
    fn test_heading_optional() {
        let result = validate_translation(
            "Just some text without heading.",
            ValidationOptions {
                require_heading: false,
            },
        );
        assert!(result.is_valid());
    }
}
