pub mod book;
pub mod config;
pub mod glossary;
pub mod io;
pub mod output;
pub mod state;
pub mod translate;
pub mod ui;
pub mod validate;

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use clap::Subcommand;

#[derive(Clone, Debug, PartialEq, Eq, clap::ValueEnum, serde::Serialize, serde::Deserialize)]
pub enum RerunMode {
    All,
    Glossary,
    Source,
}

impl std::fmt::Display for RerunMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RerunMode::All => write!(f, "all"),
            RerunMode::Glossary => write!(f, "glossary"),
            RerunMode::Source => write!(f, "source"),
        }
    }
}

#[derive(Parser)]
#[command(name = "cipher")]
#[command(about = "A book translator powered by LLMs")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new book project
    #[command(
        after_long_help = "Examples:\n  cipher init my-book\n  cipher init my-book --from other-book\n  cipher init my-book --import-glossary glossary.json"
    )]
    Init {
        /// Directory to initialize
        book_dir: PathBuf,
        /// Profile to use (defaults to global default)
        #[arg(long, short)]
        profile: Option<String>,
        /// Import glossary from an existing book
        #[arg(long = "from")]
        from_book: Option<PathBuf>,
        /// Import glossary from a file
        #[arg(long)]
        import_glossary: Option<PathBuf>,
    },
    /// Translate a book
    #[command(
        after_long_help = "Examples:\n  cipher translate\n  cipher translate --dry-run\n  cipher translate --rerun=glossary\n  cipher translate --overwrite"
    )]
    Translate {
        /// Directory containing the book (defaults to current directory)
        #[arg(default_value = ".")]
        book_dir: PathBuf,
        /// Profile to use (overrides book config and global default)
        #[arg(long, short)]
        profile: Option<String>,
        /// Profile to use for repair requests (defaults to translation profile)
        #[arg(long)]
        repair_profile: Option<String>,
        /// Profile to use for glossary extraction requests (defaults to translation profile)
        #[arg(long)]
        glossary_profile: Option<String>,
        /// Overwrite existing translations (creates backups automatically)
        #[arg(long, short)]
        overwrite: bool,
        /// Stop on first error
        #[arg(long)]
        fail_fast: bool,
        /// Re-translate chapters affected by tracked changes
        ///
        /// Modes: "all" (glossary + source), "glossary", or "source".
        /// Passing --rerun with no value defaults to "all".
        #[arg(long, value_name = "MODE", default_missing_value = "all", num_args = 0..=1)]
        rerun: Option<RerunMode>,
        /// Preview translate/rerun decisions without calling providers or writing state
        #[arg(long, short)]
        dry_run: bool,
        /// Suppress non-essential output (progress and detail lines)
        #[arg(long, short)]
        quiet: bool,
        /// Show detailed per-chapter progress and glossary info
        #[arg(long, short)]
        verbose: bool,
    },
    /// Show book translation status
    #[command(after_long_help = "Examples:\n  cipher status\n  cipher status --json")]
    Status {
        /// Directory containing the book (defaults to current directory)
        #[arg(default_value = ".")]
        book_dir: PathBuf,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage glossary
    Glossary {
        #[command(subcommand)]
        command: GlossaryCommands,
    },
    /// Run diagnostics
    ///
    /// Without a book directory, checks global configuration.
    /// With a book directory, checks both global and book configuration.
    Doctor {
        #[arg(default_value = None)]
        book_dir: Option<PathBuf>,
    },
    /// Manage profiles
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },
}

#[derive(Subcommand)]
pub enum GlossaryCommands {
    /// List glossary entries
    #[command(after_long_help = "Examples:\n  cipher glossary list\n  cipher glossary list --json")]
    List {
        /// Directory containing the book (defaults to current directory)
        #[arg(default_value = ".")]
        book_dir: PathBuf,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Import glossary from file (merges into existing)
    #[command(after_long_help = "Examples:\n  cipher glossary import --file glossary.json")]
    Import {
        /// Directory containing the book (defaults to current directory)
        #[arg(default_value = ".")]
        book_dir: PathBuf,
        /// Path to glossary file (json)
        #[arg(long, short)]
        file: PathBuf,
    },
    /// Export glossary to file
    #[command(
        after_long_help = "Examples:\n  cipher glossary export --output glossary-backup.json"
    )]
    Export {
        /// Directory containing the book (defaults to current directory)
        #[arg(default_value = ".")]
        book_dir: PathBuf,
        /// Output path
        #[arg(long, short)]
        output: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum ProfileCommands {
    /// Create a new profile
    ///
    /// Interactive by default. Use flags for non-interactive/scripted creation.
    #[command(
        after_long_help = "Examples:\n  cipher profile new\n  cipher profile new --name my-profile --provider gemini --model gemini-2.5-flash --api-key-file key.txt"
    )]
    New {
        /// Profile name (skips interactive prompt)
        #[arg(long)]
        name: Option<String>,
        /// Provider name (skips interactive provider selection)
        #[arg(long)]
        provider: Option<String>,
        /// Model name (skips interactive model prompt)
        #[arg(long)]
        model: Option<String>,
        /// Key label to assign (skips interactive key selection)
        #[arg(long)]
        key_label: Option<String>,
        /// Read API key from file (skips interactive key input)
        #[arg(long)]
        api_key_file: Option<PathBuf>,
        /// Set as default profile
        #[arg(long)]
        set_default: Option<bool>,
        /// Disable interactive prompts (fail if required flags are missing)
        #[arg(long = "no-input")]
        no_input: bool,
    },
    /// List available profiles
    #[command(after_long_help = "Examples:\n  cipher profile list\n  cipher profile list --json")]
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show profile details
    #[command(
        after_long_help = "Examples:\n  cipher profile show my-profile\n  cipher profile show my-profile --json"
    )]
    Show {
        /// Profile name
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Set the default profile
    SetDefault {
        /// Profile name
        name: String,
    },
    /// Test a profile
    Test {
        /// Profile name (defaults to default)
        name: Option<String>,
    },
}

fn run_profile_command(
    config: &mut config::GlobalConfig,
    command: ProfileCommands,
    no_input: bool,
) -> anyhow::Result<()> {
    config::cli::run_profile_command(config, command, no_input)
}

fn load_global_config() -> anyhow::Result<config::GlobalConfig> {
    config::GlobalConfig::load().context("Failed to load global config")
}

fn run_init_command(
    book_dir: PathBuf,
    profile: Option<String>,
    from_book: Option<PathBuf>,
    import_glossary: Option<PathBuf>,
) -> anyhow::Result<()> {
    let report = book::init_book(
        &book_dir,
        profile.as_deref(),
        from_book.as_deref(),
        import_glossary.as_deref(),
    )
    .with_context(|| format!("Failed to initialize book at {}", book_dir.display()))?;

    output::stderr_status("Book initialized");
    output::stderr_detail_kv("Directory", report.book_dir.display());
    if !report.created_dirs.is_empty() {
        output::stderr_status("Created directories:");
        for dir in &report.created_dirs {
            output::stderr_detail(format!("{}/", dir));
        }
    }
    if !report.created_files.is_empty() {
        output::stderr_status("Created files:");
        for file in &report.created_files {
            output::stderr_detail(file);
        }
    }
    if !report.skipped_files.is_empty() {
        output::stderr_status("Already present:");
        for file in &report.skipped_files {
            output::stderr_detail(file);
        }
    }
    if let Some(src) = report.imported_glossary {
        output::stderr_detail_kv("Imported glossary", src.display());
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_translate_command(
    book_dir: PathBuf,
    profile: Option<String>,
    repair_profile: Option<String>,
    glossary_profile: Option<String>,
    overwrite: bool,
    fail_fast: bool,
    rerun: Option<RerunMode>,
    dry_run: bool,
) -> anyhow::Result<i32> {
    let options = translate::TranslateOptions {
        profile,
        repair_profile,
        glossary_profile,
        overwrite,
        fail_fast,
        rerun,
        dry_run,
    };

    translate::translate_book(&book_dir, options).await
}

fn run_status_command(book_dir: PathBuf, json: bool) -> anyhow::Result<()> {
    ui::status::show_status(&book_dir, json)
}

fn run_glossary_command(command: GlossaryCommands) -> anyhow::Result<()> {
    match command {
        GlossaryCommands::List { book_dir, json } => glossary::cli::list_glossary(&book_dir, json),
        GlossaryCommands::Import { book_dir, file } => {
            glossary::cli::import_glossary(&book_dir, &file)
        }
        GlossaryCommands::Export { book_dir, output } => {
            glossary::cli::export_glossary(&book_dir, &output)
        }
    }
}

fn run_doctor_command(book_dir: Option<PathBuf>) -> anyhow::Result<()> {
    let config = load_global_config()?;

    if let Some(dir) = book_dir {
        book::doctor::run_book_doctor(&dir, &config);
        Ok(())
    } else {
        config::profile::run_global_doctor(&config)
    }
}

fn run_profile_subcommand(command: ProfileCommands, no_input: bool) -> anyhow::Result<()> {
    let mut config = load_global_config()?;
    run_profile_command(&mut config, command, no_input)
}

pub async fn run_command(command: Commands) -> anyhow::Result<i32> {
    match command {
        Commands::Init {
            book_dir,
            profile,
            from_book,
            import_glossary,
        } => {
            run_init_command(book_dir, profile, from_book, import_glossary)?;
            Ok(0)
        }
        Commands::Translate {
            book_dir,
            profile,
            repair_profile,
            glossary_profile,
            overwrite,
            fail_fast,
            rerun,
            dry_run,
            quiet,
            verbose,
        } => {
            output::set_quiet(quiet);
            output::set_verbose(verbose);
            run_translate_command(
                book_dir,
                profile,
                repair_profile,
                glossary_profile,
                overwrite,
                fail_fast,
                rerun,
                dry_run,
            )
            .await
        }
        Commands::Status { book_dir, json } => {
            run_status_command(book_dir, json)?;
            Ok(0)
        }
        Commands::Glossary { command } => {
            run_glossary_command(command)?;
            Ok(0)
        }
        Commands::Doctor { book_dir } => {
            run_doctor_command(book_dir)?;
            Ok(0)
        }
        Commands::Profile { command } => {
            let no_input = match &command {
                ProfileCommands::New { no_input, .. } => *no_input,
                _ => false,
            };
            run_profile_subcommand(command, no_input)?;
            Ok(0)
        }
    }
}

pub fn exit_with_error(message: impl std::fmt::Display) -> ! {
    output::stderr_error(message);
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rerun_mode_display() {
        assert_eq!(RerunMode::All.to_string(), "all");
        assert_eq!(RerunMode::Glossary.to_string(), "glossary");
        assert_eq!(RerunMode::Source.to_string(), "source");
    }
}
