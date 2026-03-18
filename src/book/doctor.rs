use std::path::Path;

use crate::book::BookLayout;
use crate::config::{self, GlobalConfig};
use crate::output::{detail, detail_kv};

pub fn run_book_doctor(dir: &Path, config: &GlobalConfig) {
    let layout = BookLayout::discover(dir);

    println!("Book doctor");
    detail_kv("Book directory", layout.paths.root.display());

    println!("Configuration");
    detail_kv(
        "config.json",
        format_path_status(&layout.paths.config_json, layout.exists.config_json),
    );

    println!("Content");
    detail_kv(
        "raw/",
        format_path_status(&layout.paths.raw_dir, layout.exists.raw_dir),
    );

    let effective_out = layout.effective_out_dir();
    let is_legacy = layout.is_using_legacy_out();

    detail_kv(
        "tl/",
        format_path_status(&layout.paths.out_dir, layout.exists.out_dir),
    );
    if layout.exists.legacy_out_dir {
        detail_kv(
            "translated/",
            format!(
                "{}; legacy output directory",
                format_path_status(&layout.paths.legacy_out_dir, layout.exists.legacy_out_dir)
            ),
        );
    }

    if is_legacy {
        detail("Using legacy output directory: translated/");
    } else {
        detail_kv("Effective output", effective_out.display());
    }

    println!("Glossary and style");
    detail_kv(
        "glossary.json",
        format_path_status(&layout.paths.glossary_json, layout.exists.glossary_json),
    );
    detail_kv(
        "style.md",
        format_path_status(&layout.paths.style_md, layout.exists.style_md),
    );

    println!("Tool state");
    detail_kv(
        ".cipher/",
        format_path_status(&layout.paths.state_dir, layout.exists.state_dir),
    );

    if layout.is_valid_book() {
        println!("Book layout looks valid");
    } else {
        println!("Book layout has issues");
        if !layout.exists.root_dir {
            detail("Book directory does not exist");
        }
        if !layout.exists.raw_dir {
            detail("raw/ directory is missing");
        }
    }

    print_profile_info(&layout, config);
}

fn print_profile_info(layout: &BookLayout, config: &GlobalConfig) {
    println!("Profile configuration");
    let book_config = crate::book::load_book_config(&layout.paths.config_json).unwrap_or_default();
    let profile_name = config.effective_profile_name(book_config.profile.as_deref());

    if let Some(name) = profile_name {
        match config.resolve_profile(name) {
            Some(profile) => {
                detail_kv("Profile", name);
                let is_default = config.default_profile.as_ref() == Some(&name.to_string());
                if is_default {
                    detail("Using the default profile");
                }
                if book_config.profile.is_some() {
                    detail("Profile is set in book config");
                }
                detail_kv("Provider", &profile.provider);
                detail_kv("Model", &profile.model);

                let validation = config::validate_profile(config, name);
                if validation.is_valid() {
                    detail("Profile configuration is valid");
                } else {
                    detail("Profile configuration has errors");
                    for err in &validation.errors {
                        detail(err);
                    }
                }
            }
            None => {
                detail_kv("Profile", format!("{} (not found)", name));
                if book_config.profile.is_some() {
                    detail("Profile is set in book config");
                }
            }
        }
    } else {
        detail("No profile configured");
        detail("Run: cipher profile new");
    }
}

fn format_path_status(path: &Path, exists: bool) -> String {
    let status = if exists { "exists" } else { "missing" };
    format!("{} ({})", status, path.display())
}
