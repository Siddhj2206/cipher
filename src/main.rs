use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod book;

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
        /// Directory containing the book
        book_dir: PathBuf,
        /// Overwrite existing translations
        #[arg(long)]
        overwrite: bool,
        /// Overwrite only failed/bad translations
        #[arg(long)]
        overwrite_bad: bool,
        /// Create backups on overwrite
        #[arg(long, default_value = "true")]
        backup: bool,
        /// Stop on first error
        #[arg(long)]
        fail_fast: bool,
    },
    /// Show book translation status
    Status {
        /// Directory containing the book
        book_dir: PathBuf,
    },
    /// Retry failed chapters
    RetryFailed {
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
    /// Interactive configuration
    Configure,
}

#[derive(Subcommand)]
enum GlossaryCommands {
    /// List glossary entries
    List {
        /// Directory containing the book
        book_dir: PathBuf,
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
    },
    /// Review pending entries interactively
    Review {
        /// Directory containing the book
        book_dir: PathBuf,
    },
    /// Approve a glossary entry
    Approve {
        /// Directory containing the book
        book_dir: PathBuf,
        /// Entry ID or term to approve
        entry: String,
    },
    /// Reject a glossary entry
    Reject {
        /// Directory containing the book
        book_dir: PathBuf,
        /// Entry ID or term to reject
        entry: String,
    },
    /// Import glossary from file
    Import {
        /// Directory containing the book
        book_dir: PathBuf,
        /// Path to glossary file (json or txt)
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

fn run_book_doctor(dir: &PathBuf) {
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
}

fn run_global_doctor() {
    println!("Running global doctor...");
    println!();
    println!("Global configuration: Not yet implemented (Feature 5)");
    println!("Use: cipher doctor <bookDir> to check a book layout");
}

fn format_path_status(path: &PathBuf, exists: bool) -> String {
    let status = if exists { "exists" } else { "missing" };
    format!("({}) {}", status, path.display())
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { .. } => {
            println!("TODO: Initialize book project");
        }
        Commands::Translate { .. } => {
            println!("TODO: Translate book");
        }
        Commands::Status { .. } => {
            println!("TODO: Show book status");
        }
        Commands::RetryFailed { .. } => {
            println!("TODO: Retry failed chapters");
        }
        Commands::Glossary { .. } => {
            println!("TODO: Manage glossary");
        }
        Commands::Doctor { book_dir } => {
            if let Some(dir) = book_dir {
                run_book_doctor(&dir);
            } else {
                run_global_doctor();
            }
        }
        Commands::Profile { .. } => {
            println!("TODO: Manage profiles");
        }
        Commands::Configure => {
            println!("TODO: Interactive configuration");
        }
    }
}
