use crate::config::profile::{generate_unique_key_label, provider_display_name};
use crate::config::{ApiKey, GlobalConfig, ProviderConfig, ProviderKind};
use crate::error::{Error, Result};
use crate::output::{stderr_detail, stderr_detail_kv};
use inquire::ui::{Color, ErrorMessageRenderConfig, RenderConfig, StyleSheet, Styled};
use inquire::{Confirm, InquireError, Password, Select, Text};

pub fn set_cipher_theme() {
    inquire::set_global_render_config(cipher_render_config());
}

fn cipher_render_config() -> RenderConfig<'static> {
    RenderConfig::default_colored()
        .with_prompt_prefix(Styled::new("?").with_fg(Color::LightGreen))
        .with_answered_prompt_prefix(Styled::new(">").with_fg(Color::LightGreen))
        .with_error_message(
            ErrorMessageRenderConfig::default_colored()
                .with_prefix(Styled::new("#").with_fg(Color::LightRed)),
        )
        .with_help_message(StyleSheet::new().with_fg(Color::LightYellow))
        .with_highlighted_option_prefix(Styled::new(">").with_fg(Color::LightGreen))
        .with_selected_option(Some(StyleSheet::new().with_fg(Color::LightGreen)))
        .with_canceled_prompt_indicator(Styled::new("<canceled>").with_fg(Color::DarkRed))
}

fn handle_inquire_error<T>(result: std::result::Result<T, InquireError>) -> Result<T> {
    match result {
        Ok(val) => Ok(val),
        Err(InquireError::OperationInterrupted | InquireError::OperationCanceled) => {
            std::process::exit(0);
        }
        Err(e) => Err(Error::Validation {
            message: format!("Failed to read input: {e}"),
        }),
    }
}

pub fn prompt_text(prompt: &str, default: Option<&str>, help: Option<&str>) -> Result<String> {
    let mut input = Text::new(prompt);
    if let Some(default) = default {
        input = input.with_default(default);
    }
    if let Some(help) = help {
        input = input.with_help_message(help);
    }
    handle_inquire_error(input.prompt())
}

pub fn prompt_password(prompt: &str) -> Result<String> {
    handle_inquire_error(Password::new(prompt).without_confirmation().prompt())
}

pub fn prompt_select(prompt: &str, items: &[&str]) -> Result<usize> {
    let list = items.to_vec();
    let result = handle_inquire_error(Select::new(prompt, list).prompt())?;
    items
        .iter()
        .position(|&x| x == result)
        .ok_or_else(|| Error::State("Selected item not found in list".to_string()))
}

pub fn prompt_confirm(prompt: &str, default: bool) -> Result<bool> {
    handle_inquire_error(Confirm::new(prompt).with_default(default).prompt())
}

pub fn prompt_profile_name(config: &GlobalConfig) -> Result<String> {
    loop {
        let profile_name = prompt_text("Profile name", None, None)?;

        if profile_name.is_empty() {
            stderr_detail("Profile name cannot be empty. Please try again.");
            continue;
        }

        if config.profiles.contains_key(&profile_name) {
            let overwrite = prompt_confirm(
                &format!("Profile '{profile_name}' already exists. Overwrite?"),
                false,
            )?;
            if !overwrite {
                return Err(Error::Validation {
                    message: "Cancelled.".to_string(),
                });
            }
        }

        return Ok(profile_name);
    }
}

fn select_existing_provider(config: &GlobalConfig) -> Result<Option<String>> {
    let mut existing_provider_names: Vec<String> = config.providers.keys().cloned().collect();
    existing_provider_names.sort();

    let provider_options: Vec<String> = existing_provider_names
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

    let mut all_options: Vec<&str> = provider_options.iter().map(|s| s.as_str()).collect();
    all_options.push("Create new provider");
    let selection = prompt_select("Select provider", &all_options)?;

    if selection < existing_count {
        Ok(Some(existing_provider_names[selection].clone()))
    } else {
        Ok(None)
    }
}

fn create_provider_interactive(config: &mut GlobalConfig) -> Result<String> {
    let creation_options = [
        "Gemini (built-in)",
        "OpenAI (built-in)",
        "OpenAI-compatible (custom)",
    ];
    let creation_selection = prompt_select("Provider type", &creation_options)?;

    match creation_selection {
        0 => create_builtin_provider(config, "gemini", ProviderKind::Gemini),
        1 => create_builtin_provider(config, "openai", ProviderKind::Openai),
        2 => loop {
            let name = prompt_text("Provider name (e.g., 'gemini', 'local-llm')", None, None)?;
            let name = name.trim().to_string();
            if name.is_empty() {
                stderr_detail("Provider name cannot be empty. Please try again.");
                continue;
            }
            if name.contains(' ') {
                stderr_detail("Provider name cannot contain spaces. Please try again.");
                continue;
            }

            if config.providers.contains_key(&name) {
                let overwrite = prompt_confirm(
                    &format!("Provider '{name}' already exists. Overwrite its config?"),
                    false,
                )?;
                if !overwrite {
                    return Err(Error::Validation {
                        message: "Cancelled.".to_string(),
                    });
                }
            }

            let url = prompt_text("Base URL", Some("https://api.openai.com/v1"), None)?;

            config.providers.insert(
                name.clone(),
                ProviderConfig {
                    kind: ProviderKind::OpenaiCompatible,
                    keys: Vec::new(),
                    base_url: Some(url),
                },
            );

            return Ok(name);
        },
        _ => Err(Error::State("Unexpected provider selection".to_string())),
    }
}

fn create_builtin_provider(
    config: &mut GlobalConfig,
    name: &'static str,
    kind: ProviderKind,
) -> Result<String> {
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

pub fn select_or_create_provider(config: &mut GlobalConfig) -> Result<String> {
    if let Some(name) = select_existing_provider(config)? {
        return Ok(name);
    }
    create_provider_interactive(config)
}

fn select_existing_api_key(provider_keys: &[ApiKey]) -> Result<Option<usize>> {
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

    let selection = prompt_select(
        "Select API key",
        &key_items.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
    )?;

    if selection < provider_keys.len() {
        Ok(Some(selection))
    } else {
        Ok(None)
    }
}

pub fn select_or_create_api_key(
    config: &mut GlobalConfig,
    provider_name: &str,
) -> Result<Option<String>> {
    let provider = config
        .providers
        .get_mut(provider_name)
        .ok_or_else(|| Error::Config(format!("Provider '{provider_name}' not found")))?;
    let provider_keys = &mut provider.keys;

    if let Some(selection) = select_existing_api_key(provider_keys)? {
        let key = &provider_keys[selection];
        if key.name.is_none() {
            let label = prompt_key_label(provider_keys, true)?
                .ok_or_else(|| Error::State("Key label required but not provided".to_string()))?;
            provider_keys[selection].name = Some(label.clone());
            Ok(Some(label))
        } else {
            Ok(key.name.clone())
        }
    } else {
        add_new_api_key(provider_keys)
    }
}

fn add_new_api_key(provider_keys: &mut Vec<ApiKey>) -> Result<Option<String>> {
    let api_key = prompt_password("API key")?;

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

fn prompt_key_label(existing: &[ApiKey], require_label: bool) -> Result<Option<String>> {
    loop {
        let label = prompt_text(
            "Key label (recommended, e.g., 'work', 'personal')",
            None,
            None,
        )?;

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

pub fn prompt_model() -> Result<String> {
    let model = prompt_text("Model name", Some("gpt-4o-mini"), None)?;
    let model = model.trim();
    if model.is_empty() {
        Ok("gpt-4o-mini".to_string())
    } else {
        Ok(model.to_string())
    }
}

pub fn prompt_default_profile(config: &mut GlobalConfig, profile_name: &str) -> Result<bool> {
    if config.default_profile.is_none() {
        config.default_profile = Some(profile_name.to_string());
        Ok(true)
    } else {
        let set_default = prompt_confirm("Set as default profile?", false)?;
        if set_default {
            config.default_profile = Some(profile_name.to_string());
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inquire::ui::Color;

    #[test]
    fn cipher_theme_prompt_prefix_is_green() {
        let config = cipher_render_config();
        assert_eq!(config.prompt_prefix.style.fg, Some(Color::LightGreen));
    }

    #[test]
    fn cipher_theme_answered_prefix_is_green() {
        let config = cipher_render_config();
        assert_eq!(
            config.answered_prompt_prefix.style.fg,
            Some(Color::LightGreen)
        );
    }

    #[test]
    fn cipher_theme_selected_option_is_green() {
        let config = cipher_render_config();
        let selected = config
            .selected_option
            .expect("selected_option should be set");
        assert_eq!(selected.fg, Some(Color::LightGreen));
    }

    #[test]
    fn cipher_theme_canceled_indicator_is_dark_red() {
        let config = cipher_render_config();
        assert_eq!(
            config.canceled_prompt_indicator.style.fg,
            Some(Color::DarkRed)
        );
    }
}
