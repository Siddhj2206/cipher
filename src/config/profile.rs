use anyhow::Context;
use dialoguer::{Input, Select};

use crate::config::{ApiKey, GlobalConfig, ProfileConfig, ProviderConfig};

pub fn create_profile_interactive() -> anyhow::Result<()> {
    let mut config = GlobalConfig::load()?;

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
        let confirm = dialoguer::Confirm::new()
            .with_prompt(format!(
                "Profile '{}' already exists. Overwrite?",
                profile_name
            ))
            .default(false)
            .interact()
            .context("Failed to get confirmation")?;
        if !confirm {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let provider_options = vec!["OpenAI", "OpenAI-compatible"];
    let selection = Select::new()
        .with_prompt("Select provider type")
        .items(&provider_options)
        .interact()
        .context("Failed to select provider type")?;

    let provider_name: String;
    let base_url: Option<String>;

    match selection {
        0 => {
            provider_name = "openai".to_string();
            base_url = Some("https://api.openai.com/v1".to_string());
            if !config.providers.contains_key(&provider_name) {
                config.providers.insert(
                    provider_name.clone(),
                    ProviderConfig {
                        kind: "openai".to_string(),
                        base_url: None,
                        extras: None,
                    },
                );
            }
        }
        1 => {
            provider_name = loop {
                let name: String = Input::new()
                    .with_prompt("Provider name (e.g., 'gemini', 'local-llm')")
                    .interact_text()
                    .context("Failed to get provider name")?;
                if name.is_empty() {
                    eprintln!("Provider name cannot be empty. Please try again.");
                } else if name.contains(' ') {
                    eprintln!("Provider name cannot contain spaces. Please try again.");
                } else {
                    break name;
                }
            };

            let url: String = Input::new()
                .with_prompt("Base URL")
                .default("https://api.openai.com/v1".to_string())
                .interact_text()
                .context("Failed to get base URL")?;
            base_url = Some(url.clone());

            config.providers.insert(
                provider_name.clone(),
                ProviderConfig {
                    kind: "openai_compatible".to_string(),
                    base_url: Some(url),
                    extras: None,
                },
            );
        }
        _ => unreachable!(),
    }

    let api_key = dialoguer::Password::new()
        .with_prompt("API key")
        .interact()
        .context("Failed to get API key")?;

    let key_label: String = Input::new()
        .with_prompt("Key label (optional, e.g., 'work', 'personal')")
        .allow_empty(true)
        .interact_text()
        .context("Failed to get key label")?;

    let key_label = if key_label.trim().is_empty() {
        None
    } else {
        Some(key_label.trim().to_string())
    };

    let provider_keys = config.keys.entry(provider_name.clone()).or_default();
    let api_key_entry = ApiKey {
        value: api_key,
        name: key_label.clone(),
    };
    provider_keys.push(api_key_entry);

    let model: String = Input::new()
        .with_prompt("Model name")
        .default("gpt-4o-mini".to_string())
        .interact_text()
        .context("Failed to get model name")?;

    let use_temperature = dialoguer::Confirm::new()
        .with_prompt("Set temperature?")
        .default(false)
        .interact()
        .context("Failed to get temperature preference")?;

    let temperature = if use_temperature {
        let temp: f64 = loop {
            let input: String = Input::new()
                .with_prompt("Temperature (0.0 - 2.0)")
                .default("0.2".to_string())
                .interact_text()
                .context("Failed to get temperature")?;
            match input.parse::<f64>() {
                Ok(v) if (0.0..=2.0).contains(&v) => break v,
                Ok(_) => eprintln!("Temperature must be between 0.0 and 2.0. Please try again."),
                Err(_) => eprintln!("Please enter a valid number."),
            }
        };
        Some(temp as f32)
    } else {
        None
    };

    let profile = ProfileConfig {
        provider: provider_name,
        model,
        key: key_label,
        temperature,
    };

    config.profiles.insert(profile_name.clone(), profile);

    if config.default_profile.is_none() {
        config.default_profile = Some(profile_name.clone());
        println!("Set '{}' as the default profile.", profile_name);
    } else {
        let set_default = dialoguer::Confirm::new()
            .with_prompt("Set as default profile?")
            .default(false)
            .interact()
            .context("Failed to get default preference")?;
        if set_default {
            config.default_profile = Some(profile_name.clone());
            println!("Set '{}' as the default profile.", profile_name);
        }
    }

    config.save()?;

    println!("Profile '{}' created successfully.", profile_name);
    println!("\nYou can now use it with:");
    println!("  cipher translate <bookDir> --profile {}", profile_name);

    Ok(())
}

pub fn list_profiles(config: &GlobalConfig) {
    if config.profiles.is_empty() {
        println!("No profiles configured.");
        println!("Run 'cipher profile new' to create one.");
        return;
    }

    println!("Configured profiles:\n");
    for (name, profile) in &config.profiles {
        let is_default = config.default_profile.as_ref().map(|s| s.as_str()) == Some(name);
        let marker = if is_default { " (default)" } else { "" };
        println!("{}{}", name, marker);
        println!("  Provider: {}", profile.provider);
        println!("  Model: {}", profile.model);
        if let Some(temp) = profile.temperature {
            println!("  Temperature: {}", temp);
        }
        println!();
    }
}

pub fn show_profile(config: &GlobalConfig, name: &str) {
    let Some(profile) = config.resolve_profile(name) else {
        eprintln!("Error: Profile '{}' not found", name);
        return;
    };

    let is_default = config.default_profile.as_ref().map(|s| s.as_str()) == Some(name);
    println!("Profile: {}", name);
    if is_default {
        println!("  [default profile]");
    }
    println!("  Provider: {}", profile.provider);
    println!("  Model: {}", profile.model);

    if let Some(provider) = config.resolve_provider(&profile.provider) {
        match provider.kind.as_str() {
            "openai" => println!("  Kind: OpenAI"),
            "openai_compatible" => {
                println!("  Kind: OpenAI-compatible");
                if let Some(url) = &provider.base_url {
                    println!("  Base URL: {}", url);
                }
            }
            other => println!("  Kind: {}", other),
        }
    }

    if let Some(key) = &profile.key {
        println!("  Key label: {}", key);
    }
    if let Some(temp) = profile.temperature {
        println!("  Temperature: {}", temp);
    }
}

pub fn set_default_profile(config: &mut GlobalConfig, name: &str) -> anyhow::Result<()> {
    if !config.profiles.contains_key(name) {
        anyhow::bail!("Profile '{}' not found", name);
    }
    config.default_profile = Some(name.to_string());
    config.save()?;
    println!("Set '{}' as the default profile.", name);
    Ok(())
}

pub fn test_profile(config: &GlobalConfig, name: &str) {
    use crate::config::validate_profile;

    println!("Testing profile '{}'...\n", name);

    let validation = validate_profile(config, name);

    if validation.profile_exists {
        println!("✓ Profile exists");
    } else {
        println!("✗ Profile not found");
    }

    if validation.provider_exists {
        println!("✓ Provider configured");
    } else {
        println!("✗ Provider not found or not configured");
    }

    if validation.has_key {
        println!("✓ API key configured");
    } else {
        println!("✗ No API key configured for provider");
    }

    if !validation.errors.is_empty() {
        println!("\nErrors:");
        for err in &validation.errors {
            println!("  - {}", err);
        }
    }

    if validation.is_valid() {
        println!("\n✓ Profile configuration is valid");
    } else {
        println!("\n✗ Profile configuration has errors");
    }
}

pub fn run_global_doctor(config: &GlobalConfig) {
    use crate::config::validate_profile;

    let config_path = match GlobalConfig::config_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error determining config path: {}", e);
            return;
        }
    };

    println!("Global configuration:");
    println!("  Config path: {}", config_path.display());
    println!(
        "  Config exists: {}",
        if config_path.exists() { "yes" } else { "no" }
    );
    println!();

    if config_path.exists() {
        println!("  Providers: {}", config.providers.len());
        println!("  Profiles: {}", config.profiles.len());
        if let Some(default) = &config.default_profile {
            println!("  Default profile: {}", default);
        }
        println!();

        if !config.profiles.is_empty() {
            println!("  Profile validation:");
            for name in config.profiles.keys() {
                let validation = validate_profile(config, name);
                let status = if validation.is_valid() { "✓" } else { "✗" };
                println!("    {} {}", status, name);
            }
        }
    }
}
