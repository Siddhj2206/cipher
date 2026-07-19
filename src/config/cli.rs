use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Password, Select};

use crate::ProfileCommands;
use crate::config::profile::{generate_unique_key_label, provider_display_name};
use crate::config::{ApiKey, GlobalConfig, ProfileConfig, ProviderConfig, ProviderKind};
use crate::output::{stderr_detail, stderr_detail_kv, stderr_section};

pub fn run_profile_command(
    config: &mut GlobalConfig,
    command: ProfileCommands,
    no_input: bool,
) -> Result<()> {
    match command {
        ProfileCommands::New {
            name,
            provider,
            model,
            key_label,
            api_key_file,
            set_default,
            ..
        } => {
            let has_any_flag = name.is_some()
                || provider.is_some()
                || model.is_some()
                || key_label.is_some()
                || api_key_file.is_some();

            if has_any_flag {
                super::profile::create_profile(
                    config,
                    name,
                    provider,
                    model,
                    key_label,
                    api_key_file,
                    set_default,
                )?;
            } else if no_input {
                anyhow::bail!(
                    "Interactive input required. Provide flags: --name, --provider, --model, --api-key-file"
                );
            } else {
                create_profile_interactive()?;
            }
        }
        ProfileCommands::List { json } => {
            super::profile::list_profiles(config, json);
        }
        ProfileCommands::Show { name, json } => {
            super::profile::show_profile(config, &name, json)?;
        }
        ProfileCommands::SetDefault { name } => {
            super::profile::set_default_profile(config, &name)?;
        }
        ProfileCommands::Test { name } => {
            let name = name
                .or_else(|| config.default_profile.clone())
                .ok_or_else(|| {
                    anyhow::anyhow!("No profile name provided and no default profile set")
                })?;
            super::profile::test_profile(config, &name);
        }
    }
    Ok(())
}

fn create_profile_interactive() -> Result<()> {
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

fn select_existing_provider(config: &GlobalConfig) -> anyhow::Result<Option<String>> {
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
    if existing_count == 0 {
        return Ok(None);
    }

    provider_options.push("Create new provider".to_string());
    let selection = Select::new()
        .with_prompt("Select provider")
        .items(&provider_options)
        .interact()
        .context("Failed to select provider")?;

    if selection < existing_count {
        Ok(Some(existing_provider_names[selection].clone()))
    } else {
        Ok(None)
    }
}

fn create_provider_interactive(config: &mut GlobalConfig) -> anyhow::Result<String> {
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
        0 => create_builtin_provider(config, "gemini", ProviderKind::Gemini),
        1 => create_builtin_provider(config, "openai", ProviderKind::Openai),
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
        _ => anyhow::bail!("Unexpected provider selection"),
    }
}

fn create_builtin_provider(
    config: &mut GlobalConfig,
    name: &'static str,
    kind: ProviderKind,
) -> anyhow::Result<String> {
    config
        .providers
        .entry(name.to_string())
        .or_insert(ProviderConfig {
            kind,
            keys: Vec::new(),
            base_url: None,
        });
    Ok(name.to_string())
}

fn select_or_create_provider_sectioned(config: &mut GlobalConfig) -> anyhow::Result<String> {
    if let Some(name) = select_existing_provider(config)? {
        return Ok(name);
    }
    create_provider_interactive(config)
}

fn select_existing_api_key(provider_keys: &[ApiKey]) -> anyhow::Result<Option<usize>> {
    if provider_keys.is_empty() {
        return Ok(None);
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
        Ok(Some(selection))
    } else {
        Ok(None)
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

    if let Some(selection) = select_existing_api_key(provider_keys)? {
        let key = &provider_keys[selection];
        if key.name.is_none() {
            let label = prompt_key_label(provider_keys, true)?.expect("require_label ensures Some");
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

    let label = prompt_key_label(provider_keys, false)?
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

fn prompt_key_label(existing: &[ApiKey], require_label: bool) -> anyhow::Result<Option<String>> {
    loop {
        let label: String = Input::new()
            .with_prompt("Key label (recommended, e.g., 'work', 'personal')")
            .allow_empty(!require_label)
            .interact_text()
            .context("Failed to get key label")?;

        let label = label.trim().to_string();
        if label.is_empty() {
            if require_label {
                stderr_detail("Key label cannot be empty. Please try again.");
                continue;
            }
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
