use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod book;
mod config;
mod glossary;
mod import;
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

fn run_book_doctor(dir: &PathBuf, config: &config::GlobalConfig) {
    use book::BookLayout;

    let layout = BookLayout::discover(dir);

    println!("Book directory: {}", layout.paths.root.display());
    println!(
        "Root exists: {}",
        if layout.exists.root_dir { "yes" } else { "NO" }
    );
    println!();

    println!("Configuration:");
    println!(
        "  config.json:      {}",
        format_path_status(&layout.paths.config_json, layout.exists.config_json)
    );
    println!();

    println!("Content directories:");
    println!(
        "  raw/              {}",
        format_path_status(&layout.paths.raw_dir, layout.exists.raw_dir)
    );

    let effective_out = layout.effective_out_dir();
    let is_legacy = layout.is_using_legacy_out();

    println!(
        "  tl/               {}",
        format_path_status(&layout.paths.out_dir, layout.exists.out_dir)
    );
    if layout.exists.legacy_out_dir {
        println!(
            "  translated/       {} (legacy)",
            format_path_status(&layout.paths.legacy_out_dir, layout.exists.legacy_out_dir)
        );
    }

    if is_legacy {
        println!("  Using legacy output dir: translated/");
    } else {
        println!("  Effective output: {}", effective_out.display());
    }
    println!();

    println!("Glossary and style:");
    println!(
        "  glossary.json:    {}",
        format_path_status(&layout.paths.glossary_json, layout.exists.glossary_json)
    );
    println!(
        "  style.md:         {}",
        format_path_status(&layout.paths.style_md, layout.exists.style_md)
    );
    println!();

    println!("Tool state:");
    println!(
        "  .cipher/          {}",
        format_path_status(&layout.paths.state_dir, layout.exists.state_dir)
    );
    println!();

    if layout.is_valid_book() {
        println!("Status: Valid book layout");
    } else {
        println!("Status: Invalid book layout");
        if !layout.exists.root_dir {
            eprintln!("  ERROR: Book directory does not exist");
        }
        if !layout.exists.raw_dir {
            eprintln!("  ERROR: raw/ directory is missing");
        }
    }

    println!();
    println!("Profile configuration:");
    let book_config = book::load_book_config(&layout.paths.config_json).unwrap_or_default();
    let profile_name = config.effective_profile_name(book_config.profile.as_deref());

    if let Some(name) = profile_name {
        match config.resolve_profile(name) {
            Some(profile) => {
                println!("  Profile: {}", name);
                let is_default = config.default_profile.as_ref() == Some(&name.to_string());
                if is_default {
                    println!("    [default]");
                }
                if book_config.profile.is_some() {
                    println!("    [set in book config]");
                }
                println!("  Provider: {}", profile.provider);
                println!("  Model: {}", profile.model);

                let validation = config::validate_profile(config, name);
                if validation.is_valid() {
                    println!("  Status: ✓ Valid");
                } else {
                    println!("  Status: ✗ Configuration errors");
                    for err in &validation.errors {
                        println!("    - {}", err);
                    }
                }
            }
            None => {
                println!("  Profile: {} (not found)", name);
                if book_config.profile.is_some() {
                    println!("    [set in book config]");
                }
            }
        }
    } else {
        println!("  No profile configured.");
        println!("  Run: cipher profile new");
    }
}

fn run_global_doctor(config: &config::GlobalConfig) {
    config::profile::run_global_doctor(config);
}

fn run_profile_command(
    config: &mut config::GlobalConfig,
    command: ProfileCommands,
) -> anyhow::Result<()> {
    config::cli::run_profile_command(config, command)
}

fn format_path_status(path: &PathBuf, exists: bool) -> String {
    let status = if exists { "exists" } else { "missing" };
    format!("({}) {}", status, path.display())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Import { epub_path, force } => match import::import_epub(&epub_path, force) {
            Ok(report) => {
                println!(
                    "Imported {} chapters to {}",
                    report.chapters_imported,
                    report.book_dir.display()
                );
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
                    println!("Initialized book: {}", report.book_dir.display());
                    println!();
                    if !report.created_dirs.is_empty() {
                        println!("Created directories:");
                        for dir in &report.created_dirs {
                            println!("  - {}/", dir);
                        }
                    }
                    if !report.created_files.is_empty() {
                        println!("Created files:");
                        for file in &report.created_files {
                            println!("  - {}", file);
                        }
                    }
                    if !report.skipped_files.is_empty() {
                        println!("Skipped (already exist):");
                        for file in &report.skipped_files {
                            println!("  - {}", file);
                        }
                    }
                    if let Some(src) = report.imported_glossary {
                        println!("Imported glossary from: {}", src.display());
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
        } => {
            let options = translate::TranslateOptions {
                profile,
                overwrite,
                fail_fast,
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
            if let Err(e) = run_glossary_command(command) {
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
                run_book_doctor(&dir, &config);
            } else {
                run_global_doctor(&config);
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

fn run_glossary_command(command: GlossaryCommands) -> Result<(), anyhow::Error> {
    use glossary::{load_glossary, merge_terms, save_glossary};

    match command {
        GlossaryCommands::List { book_dir } => {
            let layout = book::BookLayout::discover(&book_dir);
            let terms = load_glossary(&layout.paths.glossary_json)?;

            if terms.is_empty() {
                println!("No glossary entries found.");
            } else {
                println!("Glossary entries ({}):\n", terms.len());
                for (i, term) in terms.iter().enumerate() {
                    let def_preview = if term.definition.chars().count() > 60 {
                        format!(
                            "{}...",
                            term.definition.chars().take(60).collect::<String>()
                        )
                    } else {
                        term.definition.clone()
                    };
                    if let Some(ref og) = term.og_term {
                        println!("{}: {} [{}] - {}", i + 1, term.term, og, def_preview);
                    } else {
                        println!("{}: {} - {}", i + 1, term.term, def_preview);
                    }
                }
            }
        }
        GlossaryCommands::Import { book_dir, path } => {
            let layout = book::BookLayout::discover(&book_dir);
            let incoming = load_glossary(&path)?;

            if incoming.is_empty() {
                println!("Import file is empty. Nothing to import.");
                return Ok(());
            }

            let existing = load_glossary(&layout.paths.glossary_json)?;
            let (merged, added, skipped) = merge_terms(existing, incoming);

            if added > 0 {
                let mut merged_mut = merged;
                save_glossary(&layout.paths.glossary_json, &mut merged_mut)?;
                println!(
                    "Import complete: {} added, {} skipped (duplicates)",
                    added, skipped
                );
            } else {
                println!(
                    "Import complete: {} added, {} skipped (all duplicates)",
                    added, skipped
                );
            }
        }
        GlossaryCommands::Export { book_dir, path } => {
            let layout = book::BookLayout::discover(&book_dir);
            let terms = load_glossary(&layout.paths.glossary_json)?;

            let mut terms_mut = terms;
            save_glossary(&path, &mut terms_mut)?;
            println!(
                "Exported {} glossary entries to {}",
                terms_mut.len(),
                path.display()
            );
        }
    }
    Ok(())
}
