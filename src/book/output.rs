use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputConfig {
    #[serde(default)]
    pub fields: OutputFieldsConfig,
    #[serde(default)]
    pub render: OutputRenderConfig,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            fields: OutputFieldsConfig::default(),
            render: OutputRenderConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputFieldsConfig {
    #[serde(default)]
    pub chapter_number: OutputFieldConfig,
    #[serde(default = "default_optional_field")]
    pub chapter_title: OutputFieldConfig,
    #[serde(default = "default_required_content_field")]
    pub content: OutputFieldConfig,
}

impl Default for OutputFieldsConfig {
    fn default() -> Self {
        Self {
            chapter_number: OutputFieldConfig {
                required: false,
                description: Some("Chapter number when one is present".to_string()),
            },
            chapter_title: default_optional_field(),
            content: default_required_content_field(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputFieldConfig {
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Default for OutputFieldConfig {
    fn default() -> Self {
        default_optional_field()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputRenderConfig {
    #[serde(default = "default_render_template")]
    pub template: String,
}

impl Default for OutputRenderConfig {
    fn default() -> Self {
        Self {
            template: default_render_template(),
        }
    }
}

fn default_optional_field() -> OutputFieldConfig {
    OutputFieldConfig {
        required: false,
        description: None,
    }
}

fn default_required_content_field() -> OutputFieldConfig {
    OutputFieldConfig {
        required: true,
        description: Some(
            "Main translated chapter body in markdown, excluding the top heading".to_string(),
        ),
    }
}

fn default_render_template() -> String {
    "# {heading}\n\n{content}".to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StructuredChapter {
    pub chapter_number: Option<String>,
    pub chapter_title: Option<String>,
    pub content: String,
}

impl StructuredChapter {
    pub fn normalized(mut self) -> Self {
        self.chapter_number = normalize_optional(self.chapter_number.take());
        self.chapter_title = normalize_optional(self.chapter_title.take());
        self.content = self.content.trim().to_string();
        self
    }

    pub fn heading(&self) -> Option<String> {
        match (&self.chapter_number, &self.chapter_title) {
            (Some(number), Some(title)) => Some(format!("Chapter {}: {}", number, title)),
            (Some(number), None) => Some(format!("Chapter {}", number)),
            (None, Some(title)) => Some(title.clone()),
            (None, None) => None,
        }
    }
}

pub fn validate_structured_chapter(
    chapter: &StructuredChapter,
    config: &OutputConfig,
) -> Vec<String> {
    let mut errors = Vec::new();

    if config.fields.chapter_number.required && chapter.chapter_number.is_none() {
        errors.push("Missing required field: chapter_number".to_string());
    }

    if config.fields.chapter_title.required && chapter.chapter_title.is_none() {
        errors.push("Missing required field: chapter_title".to_string());
    }

    if config.fields.content.required && chapter.content.trim().is_empty() {
        errors.push("Missing required field: content".to_string());
    }

    errors
}

pub fn render_chapter_markdown(chapter: &StructuredChapter, config: &OutputConfig) -> String {
    let heading = chapter.heading().unwrap_or_default();

    config
        .render
        .template
        .replace("{heading}", &heading)
        .replace(
            "{chapter_number}",
            chapter.chapter_number.as_deref().unwrap_or(""),
        )
        .replace(
            "{chapter_title}",
            chapter.chapter_title.as_deref().unwrap_or(""),
        )
        .replace("{content}", &chapter.content)
        .trim()
        .to_string()
}

pub fn render_requires_heading(config: &OutputConfig) -> bool {
    config.render.template.trim_start().starts_with('#')
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_structured_chapter_heading() {
        let chapter = StructuredChapter {
            chapter_number: Some("12".to_string()),
            chapter_title: Some("A New Dawn".to_string()),
            content: "Body".to_string(),
        };

        assert_eq!(chapter.heading().as_deref(), Some("Chapter 12: A New Dawn"));
    }

    #[test]
    fn test_render_default_template() {
        let chapter = StructuredChapter {
            chapter_number: Some("12".to_string()),
            chapter_title: Some("A New Dawn".to_string()),
            content: "Body".to_string(),
        };

        let rendered = render_chapter_markdown(&chapter, &OutputConfig::default());
        assert_eq!(rendered, "# Chapter 12: A New Dawn\n\nBody");
    }

    #[test]
    fn test_validate_required_fields() {
        let chapter = StructuredChapter {
            chapter_number: None,
            chapter_title: None,
            content: "".to_string(),
        };
        let mut config = OutputConfig::default();
        config.fields.chapter_number.required = true;

        let errors = validate_structured_chapter(&chapter, &config);

        assert!(errors.iter().any(|e| e.contains("chapter_number")));
        assert!(errors.iter().any(|e| e.contains("content")));
    }
}
