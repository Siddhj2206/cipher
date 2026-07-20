use anyhow::Result;

use crate::ProfileCommands;
use crate::config::GlobalConfig;
use crate::output::{stderr_detail, stderr_detail_kv, stderr_section};
use crate::ui::interactive;

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
                create_profile_interactive(config)?;
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

fn create_profile_interactive(config: &mut GlobalConfig) -> Result<()> {
    stderr_section("Profile configuration");
    let profile_name = interactive::prompt_profile_name(config)?;

    stderr_section("Provider");
    let provider_name = interactive::select_or_create_provider(config)?;

    stderr_section("API key");
    let selected_key_label = interactive::select_or_create_api_key(config, &provider_name)?;

    stderr_section("Model");
    let model = interactive::prompt_model()?;

    let profile = crate::config::ProfileConfig {
        provider: provider_name,
        model,
        key: selected_key_label,
    };

    config.profiles.insert(profile_name.clone(), profile);

    eprintln!();
    let is_default = interactive::prompt_default_profile(config, &profile_name)?;

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
