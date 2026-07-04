use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Password, Select};
use serde::Serialize;
use std::path::PathBuf;

use crate::config::{ApiKey, GlobalConfig, ProfileConfig, ProviderConfig, ProviderKind};
use crate::output::{
    detail, detail_kv, section, stderr_detail, stderr_detail_kv, stderr_section,
    status,
};

fn provider_display_name(name: &str, cfg: &ProviderConfig) -> String {
    match cfg.kind {
        ProviderKind::Gemini => format!("{} (Gemini)", name),
        ProviderKind::Openai => format!("{} (OpenAI)", name),
        ProviderKind::OpenaiCompatible => {
            if let Some(url) = cfg.base_url.as_deref() {
                format!("{} (OpenAI-compatible, {})", name, url)
            } else {
                format!("{} (OpenAI-compatible)", name)
            }
        }
    }
}

fn prompt_provider_name() -> anyhow::Result<String> {
    loop {
        let name: String = Input::new()
            .with_prompt("Provider name (e.g., 'gemini', 'local-llm')")
            .interact_text()
            .context("Failed to get provider name")?;
        if name.trim().is_empty() {
            stderr_detail("Provider name cannot be empty. Please try again.");
        } else if name.contains(' ') {
            stderr_detail("Provider name cannot contain spaces. Please try again.");
        } else {
            return Ok(name.trim().to_string());
        }
    }
}

fn generate_unique_key_label(existing: &[ApiKey]) -> String {
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

fn prompt_key_label(existing: &[ApiKey], allow_empty: bool) -> anyhow::Result<Option<String>> {
    loop {
        let label: String = Input::new()
            .with_prompt("Key label (recommended, e.g., 'work', 'personal')")
            .allow_empty(allow_empty)
            .interact_text()
            .context("Failed to get key label")?;

        let label = label.trim().to_string();
        if label.is_empty() {
            return Ok(None);
        }

        if existing
            .iter()
            .any(|k| k.name.as_deref() == Some(label.as_str()))
        {
            stderr_detail(
                "That key label is already used for this provider. Please choose another.",
            );
            continue;
        }

        return Ok(Some(label));
    }
}

pub fn create_profile(
    config: &mut GlobalConfig,
    name: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    key_label: Option<String>,
    api_key_file: Option<PathBuf>,
    set_default: Option<bool>,
    no_input: bool,
) -> Result<()> {
    let has_any_flag = name.is_some()
        || provider.is_some()
        || model.is_some()
        || key_label.is_some()
        || api_key_file.is_some();

    if has_any_flag {
        create_profile_noninteractive(config, name, provider, model, key_label, api_key_file, set_default)
    } else if no_input {
        anyhow::bail!(
            "Interactive input required. Provide flags: --name, --provider, --model, --api-key-file"
        );
    } else {
        create_profile_interactive()
    }
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
    let profile_name = name.ok_or_else(|| anyhow::anyhow!("--name is required for non-interactive profile creation"))?;
    let provider = provider_name.ok_or_else(|| anyhow::anyhow!("--provider is required for non-interactive profile creation"))?;

    if profile_name.is_empty() {
        anyhow::bail!("Profile name cannot be empty");
    }
    if provider.is_empty() {
        anyhow::bail!("Provider name cannot be empty");
    }

    let model_name = model.unwrap_or_else(|| "gpt-4o-mini".to_string());

    let api_key = if let Some(ref key_file) = api_key_file {
        std::fs::read_to_string(key_file)
            .with_context(|| format!("Failed to read API key from {}", key_file.display()))?
            .trim()
            .to_string()
    } else {
        anyhow::bail!("--api-key-file is required for non-interactive profile creation");
    };

    if !config.providers.contains_key(&provider) {
        let kind = if provider == "gemini" {
            ProviderKind::Gemini
        } else if provider == "openai" {
            ProviderKind::Openai
        } else {
            ProviderKind::OpenaiCompatible
        };

        let base_url = if kind == ProviderKind::OpenaiCompatible {
            anyhow::bail!(
                "Provider '{}' not found. Create it first or use 'gemini'/'openai'.",
                provider
            );
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

    let resolved_key_label = key_label.or_else(|| Some(generate_unique_key_label(&config.providers[&provider].keys)));

    let provider_cfg = config.providers.get_mut(&provider).unwrap();
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

pub fn create_profile_interactive() -> Result<()> {
    let mut config = GlobalConfig::load()?;

    stderr_section("Profile configuration");
    let profile_name = prompt_profile_name(&config)?;

    stderr_section("Provider");
    let provider_name = select_or_create_provider_sectioned(&mut config)?;

    stderr_section("API key");
    let selected_key_label = select_or_create_api_key_sectioned(&mut config, &provider_name)?;

    stderr_section("Model");
    let model = prompt_model()?;

    let profile = ProfileConfig {
        provider: provider_name,
        model,
        key: selected_key_label,
    };

    config.profiles.insert(profile_name.clone(), profile);

    eprintln!();
    let is_default = prompt_default_profile(&mut config, &profile_name)?;

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

fn prompt_profile_name(config: &GlobalConfig) -> anyhow::Result<String> {
    let profile_name: String = Input::new()
        .with_prompt("Profile name")
        .interact_text()
        .context("Failed to get profile name")?;

    if profile_name.is_empty() {
        anyhow::bail!("Profile name cannot be empty");
    }

    if config.profiles.contains_key(&profile_name) {
        let confirm = Confirm::new()
            .with_prompt(format!(
                "Profile '{}' already exists. Overwrite?",
                profile_name
            ))
            .default(false)
            .interact()
            .context("Failed to get confirmation")?;
        if !confirm {
            anyhow::bail!("Cancelled.");
        }
    }

    Ok(profile_name)
}

fn select_or_create_provider_sectioned(config: &mut GlobalConfig) -> anyhow::Result<String> {
    let mut existing_provider_names: Vec<String> = config.providers.keys().cloned().collect();
    existing_provider_names.sort();

    let mut provider_options: Vec<String> = existing_provider_names
        .iter()
        .filter_map(|name| {
            config
                .providers
                .get(name)
                .map(|cfg| provider_display_name(name, cfg))
        })
        .collect();
    let existing_count = provider_options.len();
    provider_options.push("Create new provider".to_string());

    let selection = Select::new()
        .with_prompt("Select provider")
        .items(&provider_options)
        .interact()
        .context("Failed to select provider")?;

    if selection < existing_count {
        return Ok(existing_provider_names[selection].clone());
    }

    let creation_options = vec![
        "Gemini (built-in)",
        "OpenAI (built-in)",
        "OpenAI-compatible (custom)",
    ];
    let creation_selection = Select::new()
        .with_prompt("Provider type")
        .items(&creation_options)
        .interact()
        .context("Failed to select provider type")?;

    match creation_selection {
        0 => {
            let name = "gemini".to_string();
            config
                .providers
                .entry(name.clone())
                .or_insert(ProviderConfig {
                    kind: ProviderKind::Gemini,
                    keys: Vec::new(),
                    base_url: None,
                });
            Ok(name)
        }
        1 => {
            let name = "openai".to_string();
            config
                .providers
                .entry(name.clone())
                .or_insert(ProviderConfig {
                    kind: ProviderKind::Openai,
                    keys: Vec::new(),
                    base_url: None,
                });
            Ok(name)
        }
        2 => {
            let name = prompt_provider_name()?;

            if config.providers.contains_key(&name) {
                let confirm = Confirm::new()
                    .with_prompt(format!(
                        "Provider '{}' already exists. Overwrite its config?",
                        name
                    ))
                    .default(false)
                    .interact()
                    .context("Failed to get confirmation")?;
                if !confirm {
                    anyhow::bail!("Cancelled.");
                }
            }

            let url: String = Input::new()
                .with_prompt("Base URL")
                .default("https://api.openai.com/v1".to_string())
                .interact_text()
                .context("Failed to get base URL")?;

            config.providers.insert(
                name.clone(),
                ProviderConfig {
                    kind: ProviderKind::OpenaiCompatible,
                    keys: Vec::new(),
                    base_url: Some(url),
                },
            );

            Ok(name)
        }
        _ => unreachable!(),
    }
}

fn select_or_create_api_key_sectioned(
    config: &mut GlobalConfig,
    provider_name: &str,
) -> anyhow::Result<Option<String>> {
    let provider = config
        .providers
        .get_mut(provider_name)
        .ok_or_else(|| anyhow::anyhow!("Provider '{}' not found", provider_name))?;
    let provider_keys = &mut provider.keys;

    if provider_keys.is_empty() {
        return add_new_api_key(provider_keys);
    }

    let mut key_items: Vec<String> = provider_keys
        .iter()
        .enumerate()
        .map(|(idx, k)| {
            k.name
                .clone()
                .unwrap_or_else(|| format!("(unnamed) # {}", idx + 1))
        })
        .collect();
    key_items.push("Add new API key".to_string());

    let selection = Select::new()
        .with_prompt("Select API key")
        .items(&key_items)
        .interact()
        .context("Failed to select API key")?;

    if selection < provider_keys.len() {
        let key = &provider_keys[selection];
        if key.name.is_none() {
            let label = loop {
                let label: String = Input::new()
                    .with_prompt("Assign a label to this key")
                    .interact_text()
                    .context("Failed to get key label")?;
                let label = label.trim().to_string();
                if label.is_empty() {
                    stderr_detail("Key label cannot be empty. Please try again.");
                    continue;
                }
                if provider_keys
                    .iter()
                    .any(|k| k.name.as_deref() == Some(label.as_str()))
                {
                    stderr_detail(
                        "That key label is already used for this provider. Please choose another.",
                    );
                    continue;
                }
                break label;
            };
            provider_keys[selection].name = Some(label.clone());
            Ok(Some(label))
        } else {
            Ok(key.name.clone())
        }
    } else {
        add_new_api_key(provider_keys)
    }
}

fn add_new_api_key(provider_keys: &mut Vec<ApiKey>) -> anyhow::Result<Option<String>> {
    let api_key = Password::new()
        .with_prompt("API key")
        .interact()
        .context("Failed to get API key")?;

    let label = prompt_key_label(provider_keys, true)?
        .or_else(|| Some(generate_unique_key_label(provider_keys)));

    if label.is_some() {
        stderr_detail_kv("Assigned key label", label.as_deref().unwrap_or(""));
    }

    provider_keys.push(ApiKey {
        value: api_key,
        name: label.clone(),
    });

    Ok(label)
}

fn prompt_model() -> anyhow::Result<String> {
    let model: String = Input::new()
        .with_prompt("Model name")
        .allow_empty(true)
        .interact_text()
        .context("Failed to get model name")?;

    let model = model.trim();
    if model.is_empty() {
        Ok("gpt-4o-mini".to_string())
    } else {
        Ok(model.to_string())
    }
}

fn prompt_default_profile(config: &mut GlobalConfig, profile_name: &str) -> Result<bool> {
    if config.default_profile.is_none() {
        config.default_profile = Some(profile_name.to_string());
        Ok(true)
    } else {
        let set_default = Confirm::new()
            .with_prompt("Set as default profile?")
            .default(false)
            .interact()
            .context("Failed to get default preference")?;
        if set_default {
            config.default_profile = Some(profile_name.to_string());
            Ok(true)
        } else {
            Ok(false)
        }
    }
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

pub fn list_profiles(config: &GlobalConfig, json: bool) {
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
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        return;
    }

    if config.profiles.is_empty() {
        section("No profiles configured");
        detail("Run: cipher profile new");
        return;
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
}

pub fn show_profile(config: &GlobalConfig, name: &str, json: bool) -> Result<()> {
    let Some(profile) = config.resolve_profile(name) else {
        anyhow::bail!("Profile '{}' not found", name);
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
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
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

pub fn set_default_profile(config: &mut GlobalConfig, name: &str) -> anyhow::Result<()> {
    if !config.profiles.contains_key(name) {
        anyhow::bail!("Profile '{}' not found", name);
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
