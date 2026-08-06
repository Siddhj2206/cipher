pub mod cli;
pub mod profile;

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, ProfileConfig>,
}

impl GlobalConfig {
    pub fn config_path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "cipher")
            .ok_or_else(|| Error::Config("Failed to determine config directory".to_string()))?;
        let path = dirs.config_dir().join("config.toml");
        Ok(path)
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path).map_err(|e| {
            Error::Config(format!(
                "Failed to read config from {}: {e}",
                path.display()
            ))
        })?;
        let config: Self = toml::from_str(&content).map_err(|e| {
            Error::Config(format!(
                "Failed to parse config from {}: {e}",
                path.display()
            ))
        })?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let content = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("Failed to serialize config: {e}")))?;
        crate::io::atomic_write(&path, &content)
    }

    pub fn resolve_profile(&self, name: &str) -> Option<&ProfileConfig> {
        self.profiles.get(name)
    }

    pub fn resolve_provider(&self, provider: &str) -> Option<&ProviderConfig> {
        self.providers.get(provider)
    }

    pub fn get_provider_key_by_label(&self, provider: &str, label: Option<&str>) -> Option<&str> {
        let keys = &self.providers.get(provider)?.keys;

        if let Some(label) = label {
            keys.iter()
                .find(|k| k.name.as_deref() == Some(label))
                .map(|k| k.value.as_str())
        } else {
            keys.first().map(|k| k.value.as_str())
        }
    }

    pub fn effective_profile_name<'a>(&'a self, book_profile: Option<&'a str>) -> Option<&'a str> {
        let book_profile = book_profile.filter(|s| !s.is_empty());
        book_profile.or(self.default_profile.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Gemini,
    Openai,
    Cohere,
    OpenaiCompatible,
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderKind::Gemini => write!(f, "Gemini"),
            ProviderKind::Openai => write!(f, "OpenAI"),
            ProviderKind::Cohere => write!(f, "Cohere"),
            ProviderKind::OpenaiCompatible => write!(f, "OpenAI-compatible"),
        }
    }
}

impl ProviderKind {
    /// Lowercase identifier used in error messages and diagnostics
    /// (e.g. `gemini request failed`). OpenAI-compatible endpoints report as
    /// `openai` since they share the OpenAI client.
    pub fn slug(&self) -> &'static str {
        match self {
            ProviderKind::Gemini => "gemini",
            ProviderKind::Openai | ProviderKind::OpenaiCompatible => "openai",
            ProviderKind::Cohere => "cohere",
        }
    }

    /// Built-in provider kind for a provider name (the inverse of [`slug`]
    /// for built-ins). Unknown names return `None`.
    pub fn from_slug(name: &str) -> Option<ProviderKind> {
        match name {
            "gemini" => Some(ProviderKind::Gemini),
            "openai" => Some(ProviderKind::Openai),
            "cohere" => Some(ProviderKind::Cohere),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<ApiKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

#[derive(Debug)]
pub struct ConfigValidation {
    pub profile_exists: bool,
    pub provider_exists: bool,
    pub has_key: bool,
    pub errors: Vec<String>,
}

impl ConfigValidation {
    pub fn is_valid(&self) -> bool {
        self.profile_exists && self.provider_exists && self.has_key && self.errors.is_empty()
    }
}

pub fn validate_profile(config: &GlobalConfig, profile_name: &str) -> ConfigValidation {
    let mut errors = Vec::new();
    let mut validation = ConfigValidation {
        profile_exists: false,
        provider_exists: false,
        has_key: false,
        errors: Vec::new(),
    };

    let Some(profile) = config.resolve_profile(profile_name) else {
        errors.push(format!("Profile '{}' not found", profile_name));
        validation.errors = errors;
        return validation;
    };
    validation.profile_exists = true;

    if let Some(provider) = config.resolve_provider(&profile.provider) {
        validation.provider_exists = true;
        if provider.kind == ProviderKind::OpenaiCompatible && provider.base_url.is_none() {
            errors.push(format!(
                "OpenAI-compatible provider '{}' requires base_url",
                profile.provider
            ));
        }
    } else {
        errors.push(format!("Provider '{}' not found", profile.provider));
    }

    let selected_key = config.get_provider_key_by_label(&profile.provider, profile.key.as_deref());
    if selected_key.is_none() {
        if let Some(label) = profile.key.as_deref() {
            errors.push(format!(
                "No API key labeled '{}' configured for provider '{}'",
                label, profile.provider
            ));
        } else {
            errors.push(format!(
                "No API key configured for provider '{}'",
                profile.provider
            ));
        }
    } else {
        validation.has_key = true;
    }

    validation.errors = errors;
    validation
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> GlobalConfig {
        GlobalConfig {
            default_profile: None,
            providers: BTreeMap::new(),
            profiles: BTreeMap::new(),
        }
    }

    #[test]
    fn test_validate_profile_accepts_gemini_without_base_url() {
        let mut config = base_config();
        config.providers.insert(
            "gemini".to_string(),
            ProviderConfig {
                kind: ProviderKind::Gemini,
                keys: vec![ApiKey {
                    value: "test-key".to_string(),
                    name: Some("default".to_string()),
                }],
                base_url: None,
            },
        );
        config.profiles.insert(
            "gemini-profile".to_string(),
            ProfileConfig {
                provider: "gemini".to_string(),
                model: "gemini-2.5-flash".to_string(),
                key: Some("default".to_string()),
            },
        );

        let validation = validate_profile(&config, "gemini-profile");

        assert!(validation.is_valid(), "expected Gemini profile to be valid");
        assert!(validation.errors.is_empty());
    }

    #[test]
    fn effective_profile_name_prefers_book_profile() {
        let config = base_config();
        assert_eq!(
            config.effective_profile_name(Some("book-profile")),
            Some("book-profile")
        );
    }

    #[test]
    fn effective_profile_name_falls_back_to_default() {
        let mut config = base_config();
        config.default_profile = Some("default-profile".to_string());
        assert_eq!(config.effective_profile_name(None), Some("default-profile"));
    }

    #[test]
    fn effective_profile_name_filters_empty_book_profile() {
        let mut config = base_config();
        config.default_profile = Some("default-profile".to_string());
        assert_eq!(
            config.effective_profile_name(Some("")),
            Some("default-profile")
        );
    }

    #[test]
    fn effective_profile_name_returns_none_when_no_profile_available() {
        let config = base_config();
        assert_eq!(config.effective_profile_name(None), None);
    }
}
