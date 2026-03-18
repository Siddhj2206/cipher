use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Password, Select};

use crate::config::{ApiKey, GlobalConfig, ProfileConfig, ProviderConfig, ProviderKind};
use crate::output::{detail, detail_kv, section, stderr_detail};

fn provider_display_name(name: &str, cfg: &ProviderConfig) -> String {
    match cfg.kind {
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
    // Extremely unlikely fallback.
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

pub fn create_profile_interactive() -> Result<()> {
    let mut config = GlobalConfig::load()?;

    let profile_name = prompt_profile_name(&config)?;

    let provider_name = select_or_create_provider(&mut config)?;

    let selected_key_label = select_or_create_api_key(&mut config, &provider_name)?;

    let model = prompt_model()?;

    let profile = ProfileConfig {
        provider: provider_name,
        model,
        key: selected_key_label,
    };

    config.profiles.insert(profile_name.clone(), profile);

    prompt_default_profile(&mut config, &profile_name)?;

    config.save()?;

    section("Profile created");
    detail_kv("Name", &profile_name);
    detail(format!(
        "Use it with: cipher translate <bookDir> --profile {}",
        profile_name
    ));

    Ok(())
}

fn prompt_profile_name(config: &GlobalConfig) -> anyhow::Result<String> {
    let has_profiles = !config.profiles.is_empty();
    let default_name = if has_profiles { "" } else { "default" };

    let profile_name: String = Input::new()
        .with_prompt("Profile name")
        .default(default_name.to_string())
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

fn select_or_create_provider(config: &mut GlobalConfig) -> anyhow::Result<String> {
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
    provider_options.push("Create new: OpenAI".to_string());
    provider_options.push("Create new: OpenAI-compatible".to_string());

    let selection = Select::new()
        .with_prompt("Select provider")
        .items(&provider_options)
        .interact()
        .context("Failed to select provider")?;

    if selection < existing_count {
        return Ok(existing_provider_names[selection].clone());
    }

    match selection - existing_count {
        0 => {
            let name = "openai".to_string();
            config
                .providers
                .entry(name.clone())
                .or_insert(ProviderConfig {
                    kind: ProviderKind::Openai,
                    base_url: None,
                    extras: None,
                });
            Ok(name)
        }
        1 => {
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
                    base_url: Some(url),
                    extras: None,
                },
            );

            Ok(name)
        }
        _ => unreachable!(),
    }
}

fn select_or_create_api_key(
    config: &mut GlobalConfig,
    provider_name: &str,
) -> anyhow::Result<Option<String>> {
    let provider_keys = config.keys.entry(provider_name.to_string()).or_default();

    if !provider_keys.is_empty() {
        let key_mode_options = vec!["Use existing API key", "Add new API key"];
        let key_mode = Select::new()
            .with_prompt("API key")
            .items(&key_mode_options)
            .interact()
            .context("Failed to select API key mode")?;

        match key_mode {
            0 => select_existing_key(provider_keys),
            1 => add_new_api_key(provider_keys),
            _ => unreachable!(),
        }
    } else {
        add_new_api_key(provider_keys)
    }
}

fn select_existing_key(provider_keys: &mut [ApiKey]) -> anyhow::Result<Option<String>> {
    let key_items: Vec<String> = provider_keys
        .iter()
        .enumerate()
        .map(|(idx, k)| {
            k.name
                .clone()
                .unwrap_or_else(|| format!("(unnamed) #{}", idx + 1))
        })
        .collect();
    let key_idx = Select::new()
        .with_prompt("Select existing key")
        .items(&key_items)
        .interact()
        .context("Failed to select key")?;

    if provider_keys[key_idx].name.is_none() {
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
        provider_keys[key_idx].name = Some(label.clone());
        Ok(Some(label))
    } else {
        Ok(provider_keys[key_idx].name.clone())
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
        detail_kv("Assigned key label", label.as_deref().unwrap_or(""));
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
        .default("gpt-4o-mini".to_string())
        .interact_text()
        .context("Failed to get model name")?;

    Ok(model)
}

fn prompt_default_profile(config: &mut GlobalConfig, profile_name: &str) -> Result<()> {
    if config.default_profile.is_none() {
        config.default_profile = Some(profile_name.to_string());
        detail_kv("Default profile", profile_name);
    } else {
        let set_default = Confirm::new()
            .with_prompt("Set as default profile?")
            .default(false)
            .interact()
            .context("Failed to get default preference")?;
        if set_default {
            config.default_profile = Some(profile_name.to_string());
            detail_kv("Default profile", profile_name);
        }
    }
    Ok(())
}

pub fn list_profiles(config: &GlobalConfig) {
    if config.profiles.is_empty() {
        section("No profiles configured");
        detail("Run: cipher profile new");
        return;
    }

    section("Profiles");
    for (name, profile) in &config.profiles {
        println!("{}", name);
        if config.default_profile.as_deref() == Some(name) {
            detail("Default profile");
        }
        detail_kv("Provider", &profile.provider);
        detail_kv("Model", &profile.model);
    }
}

pub fn show_profile(config: &GlobalConfig, name: &str) -> Result<()> {
    let Some(profile) = config.resolve_profile(name) else {
        anyhow::bail!("Profile '{}' not found", name);
    };

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
    section("Default profile updated");
    detail_kv("Profile", name);
    Ok(())
}

pub fn test_profile(config: &GlobalConfig, name: &str) {
    use crate::config::validate_profile;

    section("Profile test");
    detail_kv("Name", name);

    let validation = validate_profile(config, name);

    detail_kv(
        "Profile",
        if validation.profile_exists {
            "found"
        } else {
            "missing"
        },
    );
    detail_kv(
        "Provider",
        if validation.provider_exists {
            "configured"
        } else {
            "missing"
        },
    );
    detail_kv(
        "API key",
        if validation.has_key {
            "configured"
        } else {
            "missing"
        },
    );

    if !validation.errors.is_empty() {
        section("Validation errors");
        for err in &validation.errors {
            detail(err);
        }
    }

    if validation.is_valid() {
        detail("Profile configuration is valid");
    } else {
        detail("Profile configuration has errors");
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
