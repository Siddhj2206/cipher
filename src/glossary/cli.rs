use std::path::Path;

use anyhow::Result;

use crate::book::BookLayout;
use crate::glossary::{load_glossary, merge_terms, save_glossary};
use crate::output::{detail, detail_kv};

pub fn list_glossary(book_dir: &Path) -> Result<()> {
    let layout = BookLayout::discover(book_dir);
    let terms = load_glossary(&layout.paths.glossary_json)?;

    if terms.is_empty() {
        println!("No glossary entries found");
        detail_kv("Path", layout.paths.glossary_json.display());
    } else {
        println!("Glossary entries");
        detail_kv("Count", terms.len());
        for term in &terms {
            let def_preview = if term.definition.chars().count() > 60 {
                format!(
                    "{}...",
                    term.definition.chars().take(60).collect::<String>()
                )
            } else {
                term.definition.clone()
            };
            if let Some(ref og) = term.og_term {
                detail(format!("{} [{}]: {}", term.term, og, def_preview));
            } else {
                detail(format!("{}: {}", term.term, def_preview));
            }
        }
    }
    Ok(())
}

pub fn import_glossary(book_dir: &Path, import_path: &Path) -> Result<()> {
    let layout = BookLayout::discover(book_dir);
    let incoming = load_glossary(import_path)?;

    if incoming.is_empty() {
        println!("Glossary import skipped");
        detail("Import file is empty");
        return Ok(());
    }

    let existing = load_glossary(&layout.paths.glossary_json)?;
    let (merged, added, skipped, _) = merge_terms(existing, incoming);

    println!("Glossary import complete");
    if added > 0 {
        save_glossary(&layout.paths.glossary_json, &merged)?;
    }
    detail_kv("Added", added);
    detail_kv("Skipped duplicates", skipped);
    Ok(())
}

pub fn export_glossary(book_dir: &Path, export_path: &Path) -> Result<()> {
    let layout = BookLayout::discover(book_dir);
    let terms = load_glossary(&layout.paths.glossary_json)?;

    save_glossary(export_path, &terms)?;
    println!("Glossary export complete");
    detail_kv("Entries", terms.len());
    detail_kv("Path", export_path.display());
    Ok(())
}
