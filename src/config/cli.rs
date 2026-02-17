use anyhow::Result;

use crate::ProfileCommands;
use crate::config::GlobalConfig;

pub fn run_profile_command(config: &mut GlobalConfig, command: ProfileCommands) -> Result<()> {
    match command {
        ProfileCommands::New => {
            super::profile::create_profile_interactive()?;
        }
        ProfileCommands::List => {
            super::profile::list_profiles(config);
        }
        ProfileCommands::Show { name } => {
            super::profile::show_profile(config, &name);
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
