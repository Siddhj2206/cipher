use std::path::PathBuf;

use clap::{Parser, Subcommand};
use output::{detail, detail_kv};

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
        /// Re-translate chapters affected by glossary changes since the last run
        #[arg(long)]
        rerun_affected_glossary: bool,
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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Import { epub_path, force } => match import::import_epub(&epub_path, force) {
            Ok(report) => {
                println!("Import complete");
                detail_kv("Book", report.book_dir.display());
                detail_kv("Chapters imported", report.chapters_imported);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        },
        Commands::Init {
            book_dir,
            profile,
            from,
            import_glossary,
        } => {
            match book::init_book(
                &book_dir,
                profile.as_deref(),
                from.as_deref(),
                import_glossary.as_deref(),
            ) {
                Ok(report) => {
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
                }
                Err(e) => {
                    eprintln!("Error initializing book: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Translate {
            book_dir,
            profile,
            overwrite,
            fail_fast,
            rerun_affected_glossary,
        } => {
            let options = translate::TranslateOptions {
                profile,
                overwrite,
                fail_fast,
                rerun_affected_glossary,
            };

            if let Err(e) = translate::translate_book(&book_dir, options).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Status { book_dir } => {
            if let Err(e) = state::status::show_status(&book_dir) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Glossary { command } => {
            let result = match command {
                GlossaryCommands::List { book_dir } => glossary::cli::list_glossary(&book_dir),
                GlossaryCommands::Import { book_dir, path } => {
                    glossary::cli::import_glossary(&book_dir, &path)
                }
                GlossaryCommands::Export { book_dir, path } => {
                    glossary::cli::export_glossary(&book_dir, &path)
                }
            };
            if let Err(e) = result {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Doctor { book_dir } => {
            let config = match config::GlobalConfig::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error loading global config: {}", e);
                    std::process::exit(1);
                }
            };

            if let Some(dir) = book_dir {
                book::doctor::run_book_doctor(&dir, &config);
            } else {
                if let Err(e) = config::profile::run_global_doctor(&config) {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Profile { command } => {
            let mut config = match config::GlobalConfig::load() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error loading global config: {}", e);
                    std::process::exit(1);
                }
            };

            if let Err(e) = run_profile_command(&mut config, command) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
