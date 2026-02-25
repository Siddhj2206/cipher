use std::path::Path;

use crate::book::BookLayout;
use crate::config::{self, GlobalConfig};

pub fn run_book_doctor(dir: &Path, config: &GlobalConfig) {
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
    print_profile_info(&layout, config);
}

fn print_profile_info(layout: &BookLayout, config: &GlobalConfig) {
    println!("Profile configuration:");
    let book_config = crate::book::load_book_config(&layout.paths.config_json).unwrap_or_default();
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
                    println!("  Status: Valid");
                } else {
                    println!("  Status: Configuration errors");
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

fn format_path_status(path: &Path, exists: bool) -> String {
    let status = if exists { "exists" } else { "missing" };
    format!("({}) {}", status, path.display())
}
