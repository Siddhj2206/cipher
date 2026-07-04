use anyhow::Result;

use crate::ProfileCommands;
use crate::config::GlobalConfig;

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
            super::profile::create_profile(
                config,
                name,
                provider,
                model,
                key_label,
                api_key_file,
                set_default,
                no_input,
            )?;
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
