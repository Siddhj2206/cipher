pub fn validate_translation(text: &str) -> ValidationResult {
    let mut errors = Vec::new();

    if text.trim().is_empty() {
        errors.push("Translation is empty".to_string());
    }

    let trimmed = text.trim_start();
    if !trimmed.starts_with('#') {
        errors.push("Translation must start with a heading (#)".to_string());
    } else {
        let first_line = trimmed.lines().next().unwrap_or("");
        if !is_valid_chapter_heading(first_line) {
            errors.push(format!(
                "Chapter heading must be in format '# Chapter X: Title' or '# Chapter X', got: {}",
                first_line
            ));
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
        ("$ref", "Schema pattern detected: $ref"),
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
        if text.contains(pattern) {
            errors.push(msg.to_string());
        }
    }

    for (pattern, msg) in response_patterns {
        if text.contains(pattern) {
            errors.push(msg.to_string());
        }
    }

    if looks_like_json_object(text) {
        errors.push("Output appears to be raw JSON instead of markdown".to_string());
    }
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
    // Must start with "# Chapter " followed by a number
    // Optionally followed by ": Title"
    let line = line.trim();
    if !line.starts_with("# Chapter ") {
        return false;
    }

    let rest = &line[10..]; // After "# Chapter "

    // Check for "X" or "X: ..." where X is a number
    let parts: Vec<&str> = rest.splitn(2, ':').collect();
    let num_part = parts[0].trim();

    // Check if the first part is a valid number
    num_part.parse::<u32>().is_ok()
}

fn has_balanced_code_fences(text: &str) -> bool {
    let mut count = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") && !trimmed.starts_with("````") {
            count += 1;
        }
    }
    count % 2 == 0
}

#[derive(Debug, Clone)]
pub enum ValidationResult {
    Valid,
    Invalid(Vec<String>),
}

impl ValidationResult {
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
        assert!(is_valid_chapter_heading("# Chapter 10: Title Here"));
        assert!(is_valid_chapter_heading("# Chapter 42"));
    }

    #[test]
    fn test_invalid_chapter_headings() {
        assert!(!is_valid_chapter_heading("# Chapter One"));
        assert!(!is_valid_chapter_heading("# Chapter"));
        assert!(!is_valid_chapter_heading("# Title"));
        assert!(!is_valid_chapter_heading("Some text"));
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
        let result = validate_translation("{\"translation\": \"text\"}");
        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.contains("raw JSON")));

        let result = validate_translation("# Chapter 1\n\n\"type\": \"string\"");
        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.contains("Schema pattern")));

        let result = validate_translation("# Chapter 1\n\n\"new_glossary_terms\": []");
        assert!(!result.is_valid());
        assert!(
            result
                .errors()
                .iter()
                .any(|e| e.contains("Response schema leaked"))
        );

        let result = validate_translation("# Chapter 1\n\nNormal text without JSON.");
        assert!(result.is_valid());
    }

    #[test]
    fn test_valid_translation_passes() {
        let text = "# Chapter 1\n\nThis is a valid translation.";
        assert!(validate_translation(text).is_valid());
    }
}
