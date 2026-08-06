use crate::config::{ApiKey, GlobalConfig, ProfileConfig, ProviderConfig, ProviderKind};
use crate::error::{Error, Result};
use crate::output::{
    detail, detail_kv, section, status, stderr_detail, stderr_detail_kv, stderr_section,
};
use serde::Serialize;
use std::path::PathBuf;

pub(crate) fn provider_display_name(name: &str, cfg: &ProviderConfig) -> String {
    match cfg.kind {
        ProviderKind::Gemini => format!("{} (Gemini)", name),
        ProviderKind::Openai => format!("{} (OpenAI)", name),
        ProviderKind::Cohere => format!("{} (Cohere)", name),
        ProviderKind::OpenaiCompatible => {
            if let Some(url) = cfg.base_url.as_deref() {
                format!("{} (OpenAI-compatible, {})", name, url)
            } else {
                format!("{} (OpenAI-compatible)", name)
            }
        }
    }
}

pub(crate) fn generate_unique_key_label(existing: &[ApiKey]) -> String {
    for n in 1..=10_000usize {
        let candidate = format!("key-{}", n);
        if !existing
            .iter()
            .any(|k| k.name.as_deref() == Some(candidate.as_str()))
        {
            return candidate;
        }
    }
    "key".to_string()
}

#[allow(clippy::too_many_arguments)]
pub fn create_profile(
    config: &mut GlobalConfig,
    name: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    key_label: Option<String>,
    api_key_file: Option<PathBuf>,
    set_default: Option<bool>,
) -> Result<()> {
    create_profile_noninteractive(
        config,
        name,
        provider,
        model,
        key_label,
        api_key_file,
        set_default,
    )
}

fn create_profile_noninteractive(
    config: &mut GlobalConfig,
    name: Option<String>,
    provider_name: Option<String>,
    model: Option<String>,
    key_label: Option<String>,
    api_key_file: Option<PathBuf>,
    set_default: Option<bool>,
) -> Result<()> {
    let profile_name = name.ok_or_else(|| Error::Validation {
        message: "--name is required for non-interactive profile creation".to_string(),
    })?;
    let provider = provider_name.ok_or_else(|| Error::Validation {
        message: "--provider is required for non-interactive profile creation".to_string(),
    })?;

    if profile_name.is_empty() {
        return Err(Error::Validation {
            message: "Profile name cannot be empty".to_string(),
        });
    }
    if provider.is_empty() {
        return Err(Error::Validation {
            message: "Provider name cannot be empty".to_string(),
        });
    }

    let model_name = model.unwrap_or_else(|| "gpt-4o-mini".to_string());

    let api_key = if let Some(ref key_file) = api_key_file {
        std::fs::read_to_string(key_file)
            .map_err(|e| {
                Error::io(
                    format!("Failed to read API key from {}", key_file.display()),
                    e,
                )
            })?
            .trim()
            .to_string()
    } else {
        return Err(Error::Validation {
            message: "--api-key-file is required for non-interactive profile creation".to_string(),
        });
    };

    if !config.providers.contains_key(&provider) {
        let kind = ProviderKind::from_slug(&provider).unwrap_or(ProviderKind::OpenaiCompatible);

        let base_url = if kind == ProviderKind::OpenaiCompatible {
            return Err(Error::Config(format!(
                "Provider '{provider}' not found. Create it first or use 'gemini'/'openai'/'cohere'."
            )));
        } else {
            None
        };

        config.providers.insert(
            provider.clone(),
            ProviderConfig {
                kind,
                keys: Vec::new(),
                base_url,
            },
        );
    }

    let existing_keys = config
        .providers
        .get(&provider)
        .map(|cfg| cfg.keys.as_slice())
        .unwrap_or(&[]);
    let resolved_key_label = key_label.or_else(|| Some(generate_unique_key_label(existing_keys)));

    let provider_cfg = config
        .providers
        .get_mut(&provider)
        .ok_or_else(|| Error::Config(format!("Provider '{provider}' not found after creation")))?;
    provider_cfg.keys.push(ApiKey {
        value: api_key,
        name: resolved_key_label.clone(),
    });

    let profile = ProfileConfig {
        provider,
        model: model_name,
        key: resolved_key_label,
    };

    let is_default = set_default.unwrap_or_else(|| config.default_profile.is_none());
    if is_default {
        config.default_profile = Some(profile_name.clone());
    }

    config.profiles.insert(profile_name.clone(), profile);
    config.save()?;

    stderr_section("Profile created");
    stderr_detail_kv("Name", &profile_name);
    if is_default {
        stderr_detail("Default profile");
    }
    stderr_detail(format!(
        "Use it with: cipher translate --profile {}",
        profile_name
    ));

    Ok(())
}

#[derive(Serialize)]
struct ProfileListOutput {
    profiles: Vec<ProfileListEntry>,
}

#[derive(Serialize)]
struct ProfileListEntry {
    name: String,
    is_default: bool,
    provider: String,
    model: String,
}

#[derive(Serialize)]
struct ProfileShowOutput {
    name: String,
    is_default: bool,
    provider: String,
    model: String,
    provider_kind: Option<String>,
    base_url: Option<String>,
    key_label: Option<String>,
}

pub fn list_profiles(config: &GlobalConfig, json: bool) -> Result<()> {
    if json {
        let output = ProfileListOutput {
            profiles: config
                .profiles
                .iter()
                .map(|(name, profile)| ProfileListEntry {
                    name: name.clone(),
                    is_default: config.default_profile.as_deref() == Some(name),
                    provider: profile.provider.clone(),
                    model: profile.model.clone(),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if config.profiles.is_empty() {
        section("No profiles configured");
        detail("Run: cipher profile new");
        return Ok(());
    }

    section("Profiles");
    for (name, profile) in &config.profiles {
        status(name);
        if config.default_profile.as_deref() == Some(name) {
            detail("Default profile");
        }
        detail_kv("Provider", &profile.provider);
        detail_kv("Model", &profile.model);
    }
    Ok(())
}

pub fn show_profile(config: &GlobalConfig, name: &str, json: bool) -> Result<()> {
    let Some(profile) = config.resolve_profile(name) else {
        return Err(Error::ProfileNotFound {
            name: name.to_string(),
        });
    };

    if json {
        let provider_kind = config
            .resolve_provider(&profile.provider)
            .map(|p| p.kind.to_string());
        let base_url = config
            .resolve_provider(&profile.provider)
            .and_then(|p| p.base_url.clone());
        let output = ProfileShowOutput {
            name: name.to_string(),
            is_default: config.default_profile.as_deref() == Some(name),
            provider: profile.provider.clone(),
            model: profile.model.clone(),
            provider_kind,
            base_url,
            key_label: profile.key.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    section(format!("Profile {}", name));
    if config.default_profile.as_deref() == Some(name) {
        detail("Default profile");
    }
    detail_kv("Provider", &profile.provider);
    detail_kv("Model", &profile.model);

    if let Some(provider) = config.resolve_provider(&profile.provider) {
        detail_kv("Provider kind", &provider.kind);
        if let Some(url) = &provider.base_url {
            detail_kv("Base URL", url);
        }
    }

    if let Some(key) = &profile.key {
        detail_kv("Key label", key);
    }

    Ok(())
}

pub fn set_default_profile(config: &mut GlobalConfig, name: &str) -> Result<()> {
    if !config.profiles.contains_key(name) {
        return Err(Error::ProfileNotFound {
            name: name.to_string(),
        });
    }
    config.default_profile = Some(name.to_string());
    config.save()?;
    stderr_section("Default profile updated");
    stderr_detail_kv("Profile", name);
    Ok(())
}

pub fn test_profile(config: &GlobalConfig, name: &str) {
    use crate::config::validate_profile;

    stderr_section("Profile test");
    stderr_detail_kv("Name", name);

    let validation = validate_profile(config, name);

    stderr_detail_kv(
        "Profile",
        if validation.profile_exists {
            "found"
        } else {
            "missing"
        },
    );
    stderr_detail_kv(
        "Provider",
        if validation.provider_exists {
            "configured"
        } else {
            "missing"
        },
    );
    stderr_detail_kv(
        "API key",
        if validation.has_key {
            "configured"
        } else {
            "missing"
        },
    );

    if !validation.errors.is_empty() {
        stderr_section("Validation errors");
        for err in &validation.errors {
            stderr_detail(err);
        }
    }

    if validation.is_valid() {
        stderr_detail("Profile configuration is valid");
    } else {
        stderr_detail("Profile configuration has errors");
    }
}

pub fn run_global_doctor(config: &GlobalConfig) -> Result<()> {
    use crate::config::validate_profile;

    let config_path = GlobalConfig::config_path()?;

    section("Global configuration");
    detail_kv("Config path", config_path.display());
    detail_kv(
        "Config exists",
        if config_path.exists() { "yes" } else { "no" },
    );

    if config_path.exists() {
        detail_kv("Providers", config.providers.len());
        detail_kv("Profiles", config.profiles.len());
        if let Some(default) = &config.default_profile {
            detail_kv("Default profile", default);
        }

        if !config.profiles.is_empty() {
            section("Profile validation");
            for name in config.profiles.keys() {
                let validation = validate_profile(config, name);
                detail_kv(
                    name,
                    if validation.is_valid() {
                        "valid"
                    } else {
                        "has errors"
                    },
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn empty_config() -> GlobalConfig {
        GlobalConfig {
            default_profile: None,
            providers: BTreeMap::new(),
            profiles: BTreeMap::new(),
        }
    }

    // -- generate_unique_key_label --

    #[test]
    fn key_label_no_existing() {
        assert_eq!(generate_unique_key_label(&[]), "key-1");
    }

    #[test]
    fn key_label_skips_existing_labels() {
        let keys = vec![
            ApiKey {
                value: "v1".into(),
                name: Some("key-1".into()),
            },
            ApiKey {
                value: "v2".into(),
                name: Some("key-2".into()),
            },
        ];
        assert_eq!(generate_unique_key_label(&keys), "key-3");
    }

    #[test]
    fn key_label_ignores_unnamed_keys() {
        let keys = vec![ApiKey {
            value: "v1".into(),
            name: None,
        }];
        assert_eq!(generate_unique_key_label(&keys), "key-1");
    }

    #[test]
    fn key_label_fills_gap() {
        let keys = vec![ApiKey {
            value: "v1".into(),
            name: Some("key-1".into()),
        }];
        assert_eq!(generate_unique_key_label(&keys), "key-2");
    }

    // -- create_profile non-interactive error paths (no config.save) --

    #[test]
    fn create_noninteractive_requires_name() {
        let mut cfg = empty_config();
        let err = create_profile(
            &mut cfg,
            None,
            Some("gemini".to_string()),
            None,
            None,
            None,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("--name is required"), "got: {err}");
    }

    #[test]
    fn create_noninteractive_requires_provider() {
        let mut cfg = empty_config();
        let err = create_profile(&mut cfg, Some("p".into()), None, None, None, None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--provider is required"), "got: {err}");
    }

    #[test]
    fn create_noninteractive_rejects_empty_name() {
        let mut cfg = empty_config();
        let err = create_profile(
            &mut cfg,
            Some("".into()),
            Some("gemini".to_string()),
            None,
            None,
            None,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("cannot be empty"), "got: {err}");
    }

    #[test]
    fn create_noninteractive_rejects_empty_provider() {
        let mut cfg = empty_config();
        let err = create_profile(
            &mut cfg,
            Some("p".into()),
            Some("".into()),
            None,
            None,
            None,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("cannot be empty"), "got: {err}");
    }

    #[test]
    fn create_noninteractive_requires_api_key_file() {
        let mut cfg = empty_config();
        let err = create_profile(
            &mut cfg,
            Some("p".into()),
            Some("gemini".into()),
            None,
            None,
            None,
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("--api-key-file is required"), "got: {err}");
    }

    #[test]
    fn create_noninteractive_rejects_unknown_provider() {
        let mut cfg = empty_config();
        let err = create_profile(
            &mut cfg,
            Some("p".into()),
            Some("custom".into()),
            None,
            None,
            Some(PathBuf::from("/dev/null")),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("not found"), "got: {err}");
    }

    // -- smoke tests for output / query functions (no filesystem writes) --

    #[test]
    fn list_profiles_empty_does_not_panic() {
        assert!(list_profiles(&empty_config(), false).is_ok());
        assert!(list_profiles(&empty_config(), true).is_ok());
    }

    #[test]
    fn show_profile_not_found_errors() {
        let err = show_profile(&empty_config(), "nope", false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[test]
    fn set_default_profile_not_found_errors_without_saving() {
        let mut cfg = empty_config();
        let err = set_default_profile(&mut cfg, "nope")
            .unwrap_err()
            .to_string();
        assert!(err.contains("not found"), "got: {err}");
        // config was not modified and save() was not called
        assert!(cfg.default_profile.is_none());
    }

    #[test]
    fn test_profile_nonexistent_does_not_panic() {
        let mut cfg = empty_config();
        cfg.providers.insert(
            "gemini".into(),
            ProviderConfig {
                kind: ProviderKind::Gemini,
                keys: vec![ApiKey {
                    value: "k".into(),
                    name: Some("key-1".into()),
                }],
                base_url: None,
            },
        );
        cfg.profiles.insert(
            "my-profile".into(),
            ProfileConfig {
                provider: "gemini".into(),
                model: "m".into(),
                key: Some("key-1".into()),
            },
        );
        test_profile(&cfg, "my-profile");
    }

    #[test]
    fn run_global_doctor_empty_does_not_panic() {
        let result = run_global_doctor(&empty_config());
        assert!(result.is_ok());
    }
}
