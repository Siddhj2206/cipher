use std::path::Path;

use anyhow::Result;

use crate::book::BookLayout;
use crate::glossary::{load_glossary, merge_terms, save_glossary};

pub fn list_glossary(book_dir: &Path) -> Result<()> {
    let layout = BookLayout::discover(book_dir);
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
    Ok(())
}

pub fn import_glossary(book_dir: &Path, import_path: &Path) -> Result<()> {
    let layout = BookLayout::discover(book_dir);
    let incoming = load_glossary(import_path)?;

    if incoming.is_empty() {
        println!("Import file is empty. Nothing to import.");
        return Ok(());
    }

    let existing = load_glossary(&layout.paths.glossary_json)?;
    let (merged, added, skipped) = merge_terms(existing, incoming);

    if added > 0 {
        save_glossary(&layout.paths.glossary_json, &merged)?;
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
    Ok(())
}

pub fn export_glossary(book_dir: &Path, export_path: &Path) -> Result<()> {
    let layout = BookLayout::discover(book_dir);
    let terms = load_glossary(&layout.paths.glossary_json)?;

    save_glossary(export_path, &terms)?;
    println!(
        "Exported {} glossary entries to {}",
        terms.len(),
        export_path.display()
    );
    Ok(())
}
