pub mod cli;
pub mod profile;

use anyhow::{Context, Result};
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
    pub keys: BTreeMap<String, Vec<ApiKey>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, ProfileConfig>,
}

impl GlobalConfig {
    pub fn config_path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "cipher", "cipher")
            .context("Failed to determine config directory")?;
        let path = dirs.config_dir().join("config.json");
        Ok(path)
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let config: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config from {}", path.display()))?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory {}", parent.display())
            })?;
        }
        let content = serde_json::to_string_pretty(self)?;
        let temp_path = path.with_extension("tmp");
        fs::write(&temp_path, content)?;
        if let Err(e) = fs::rename(&temp_path, &path) {
            let _ = fs::remove_file(&temp_path);
            return Err(e).with_context(|| format!("Failed to write config to {}", path.display()));
        }
        Ok(())
    }

    pub fn resolve_profile(&self, name: &str) -> Option<&ProfileConfig> {
        self.profiles.get(name)
    }

    pub fn resolve_provider(&self, provider: &str) -> Option<&ProviderConfig> {
        self.providers.get(provider)
    }

    pub fn get_provider_key_by_label(&self, provider: &str, label: Option<&str>) -> Option<&str> {
        let keys = self.keys.get(provider)?;

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
    Openai,
    OpenaiCompatible,
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderKind::Openai => write!(f, "OpenAI"),
            ProviderKind::OpenaiCompatible => write!(f, "OpenAI-compatible"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extras: Option<serde_json::Value>,
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
