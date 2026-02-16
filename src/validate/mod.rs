
pub fn validate_translation(text: &str) -> ValidationResult {
    let mut errors = Vec::new();

    // Check non-empty
    if text.trim().is_empty() {
        errors.push("Translation is empty".to_string());
    }

    // Check starts with # heading
    let trimmed = text.trim_start();
    if !trimmed.starts_with('#') {
        errors.push("Translation must start with a heading (#)".to_string());
    } else {
        // Check strict chapter heading format
        let first_line = trimmed.lines().next().unwrap_or("");
        if !is_valid_chapter_heading(first_line) {
            errors.push(format!(
                "Chapter heading must be in format '# Chapter X: Title' or '# Chapter X', got: {}",
                first_line
            ));
        }
    }

    // Check balanced code fences
    if !has_balanced_code_fences(text) {
        errors.push("Unbalanced code fences (```)".to_string());
    }

    if errors.is_empty() {
        ValidationResult::Valid
    } else {
        ValidationResult::Invalid(errors)
    }
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
}
