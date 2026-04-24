use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use output::{detail, detail_kv, stderr_error};

mod book;
mod config;
mod glossary;
mod import;
mod output;
mod state;
mod translate;
mod validate;

#[derive(Parser)]
#[command(name = "cipher")]
#[command(about = "A book translator powered by LLMs")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Import an EPUB file and create a new book project
    Import {
        /// Path to the EPUB file
        epub_path: PathBuf,
        /// Force re-import even if chapters exist (will prompt for confirmation)
        #[arg(long)]
        force: bool,
    },
    /// Initialize a new book project
    Init {
        /// Directory to initialize
        book_dir: PathBuf,
        /// Profile to use (defaults to global default)
        #[arg(long)]
        profile: Option<String>,
        /// Import glossary from an existing book
        #[arg(long)]
        from: Option<PathBuf>,
        /// Import glossary from a file
        #[arg(long)]
        import_glossary: Option<PathBuf>,
    },
    /// Translate a book
    Translate {
        /// Directory containing the book (defaults to current directory)
        #[arg(default_value = ".")]
        book_dir: PathBuf,
        /// Profile to use (overrides book config and global default)
        #[arg(long)]
        profile: Option<String>,
        /// Overwrite existing translations (creates backups automatically)
        #[arg(long)]
        overwrite: bool,
        /// Stop on first error
        #[arg(long)]
        fail_fast: bool,
        /// Re-translate chapters affected by tracked source or glossary changes
        #[arg(long)]
        rerun: bool,
        /// Re-translate chapters affected by glossary changes since the last run
        #[arg(long)]
        rerun_affected_glossary: bool,
        /// Re-translate chapters whose raw source changed since the last run
        #[arg(long)]
        rerun_affected_chapters: bool,
        /// Preview translate/rerun decisions without calling providers or writing state
        #[arg(long)]
        dry_run: bool,
    },
    /// Show book translation status
    Status {
        /// Directory containing the book
        book_dir: PathBuf,
    },
    /// Manage glossary
    Glossary {
        #[command(subcommand)]
        command: GlossaryCommands,
    },
    /// Run diagnostics
    Doctor {
        /// Directory containing the book (optional)
        book_dir: Option<PathBuf>,
    },
    /// Manage profiles
    Profile {
        #[command(subcommand)]
        command: ProfileCommands,
    },
}

#[derive(Subcommand)]
enum GlossaryCommands {
    /// List glossary entries
    List {
        /// Directory containing the book
        book_dir: PathBuf,
    },
    /// Import glossary from file (merges into existing)
    Import {
        /// Directory containing the book
        book_dir: PathBuf,
        /// Path to glossary file (json)
        path: PathBuf,
    },
    /// Export glossary to file
    Export {
        /// Directory containing the book
        book_dir: PathBuf,
        /// Output path
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum ProfileCommands {
    /// Create a new profile (interactive)
    New,
    /// List available profiles
    List,
    /// Show profile details
    Show {
        /// Profile name
        name: String,
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
) -> anyhow::Result<()> {
    config::cli::run_profile_command(config, command)
}

fn load_global_config() -> anyhow::Result<config::GlobalConfig> {
    config::GlobalConfig::load().context("Failed to load global config")
}

fn run_import_command(epub_path: PathBuf, force: bool) -> anyhow::Result<()> {
    let report = import::import_epub(&epub_path, force)?;

    println!("Import complete");
    detail_kv("Book", report.book_dir.display());
    detail_kv("Chapters imported", report.chapters_imported);

    Ok(())
}

fn run_init_command(
    book_dir: PathBuf,
    profile: Option<String>,
    from: Option<PathBuf>,
    import_glossary: Option<PathBuf>,
) -> anyhow::Result<()> {
    let report = book::init_book(
        &book_dir,
        profile.as_deref(),
        from.as_deref(),
        import_glossary.as_deref(),
    )
    .with_context(|| format!("Failed to initialize book at {}", book_dir.display()))?;

    println!("Book initialized");
    detail_kv("Directory", report.book_dir.display());
    if !report.created_dirs.is_empty() {
        println!("Created directories:");
        for dir in &report.created_dirs {
            detail(format!("{}/", dir));
        }
    }
    if !report.created_files.is_empty() {
        println!("Created files:");
        for file in &report.created_files {
            detail(file);
        }
    }
    if !report.skipped_files.is_empty() {
        println!("Already present:");
        for file in &report.skipped_files {
            detail(file);
        }
    }
    if let Some(src) = report.imported_glossary {
        detail_kv("Imported glossary", src.display());
    }

    Ok(())
}

async fn run_translate_command(
    book_dir: PathBuf,
    profile: Option<String>,
    overwrite: bool,
    fail_fast: bool,
    rerun: bool,
    rerun_affected_glossary: bool,
    rerun_affected_chapters: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    let options = translate::TranslateOptions {
        profile,
        overwrite,
        fail_fast,
        rerun,
        rerun_affected_glossary,
        rerun_affected_chapters,
        dry_run,
    };

    translate::translate_book(&book_dir, options).await
}

fn run_status_command(book_dir: PathBuf) -> anyhow::Result<()> {
    state::status::show_status(&book_dir)
}

fn run_glossary_command(command: GlossaryCommands) -> anyhow::Result<()> {
    match command {
        GlossaryCommands::List { book_dir } => glossary::cli::list_glossary(&book_dir),
        GlossaryCommands::Import { book_dir, path } => {
            glossary::cli::import_glossary(&book_dir, &path)
        }
        GlossaryCommands::Export { book_dir, path } => {
            glossary::cli::export_glossary(&book_dir, &path)
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

fn run_profile_subcommand(command: ProfileCommands) -> anyhow::Result<()> {
    let mut config = load_global_config()?;
    run_profile_command(&mut config, command)
}

async fn run_command(command: Commands) -> anyhow::Result<()> {
    match command {
        Commands::Import { epub_path, force } => run_import_command(epub_path, force),
        Commands::Init {
            book_dir,
            profile,
            from,
            import_glossary,
        } => run_init_command(book_dir, profile, from, import_glossary),
        Commands::Translate {
            book_dir,
            profile,
            overwrite,
            fail_fast,
            rerun,
            rerun_affected_glossary,
            rerun_affected_chapters,
            dry_run,
        } => {
            run_translate_command(
                book_dir,
                profile,
                overwrite,
                fail_fast,
                rerun,
                rerun_affected_glossary,
                rerun_affected_chapters,
                dry_run,
            )
            .await
        }
        Commands::Status { book_dir } => run_status_command(book_dir),
        Commands::Glossary { command } => run_glossary_command(command),
        Commands::Doctor { book_dir } => run_doctor_command(book_dir),
        Commands::Profile { command } => run_profile_subcommand(command),
    }
}

fn exit_with_error(message: impl std::fmt::Display) -> ! {
    stderr_error(message);
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run_command(cli.command).await {
        exit_with_error(e);
    }
}
