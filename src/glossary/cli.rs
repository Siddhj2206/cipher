use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::book::BookLayout;
use crate::glossary::{load_glossary, merge_terms, save_glossary};
use crate::output::{detail, detail_kv, status, stderr_detail, stderr_detail_kv, stderr_status};

pub fn list_glossary(book_dir: &Path, json: bool) -> Result<()> {
    let layout = BookLayout::discover(book_dir);
    let terms = load_glossary(&layout.paths.glossary_json)?;

    if json {
        let output = GlossaryListOutput {
            path: layout.paths.glossary_json.display().to_string(),
            count: terms.len(),
            terms: terms
                .iter()
                .map(|t| GlossaryEntryOutput {
                    term: t.term.clone(),
                    og_term: t.og_term.clone(),
                    definition: t.definition.clone(),
                    notes: t.notes.clone(),
                })
                .collect(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if terms.is_empty() {
        status("No glossary entries found");
        detail_kv("Path", layout.paths.glossary_json.display());
    } else {
        status("Glossary entries");
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
        stderr_status("Glossary import skipped");
        stderr_detail("Import file is empty");
        return Ok(());
    }

    let existing = load_glossary(&layout.paths.glossary_json)?;
    let (merged, added, skipped, _) = merge_terms(existing, incoming);

    stderr_status("Glossary import complete");
    if added > 0 {
        save_glossary(&layout.paths.glossary_json, &merged)?;
    }
    stderr_detail_kv("Added", added);
    stderr_detail_kv("Skipped duplicates", skipped);
    Ok(())
}

pub fn export_glossary(book_dir: &Path, export_path: &Path) -> Result<()> {
    let layout = BookLayout::discover(book_dir);
    let terms = load_glossary(&layout.paths.glossary_json)?;

    save_glossary(export_path, &terms)?;
    stderr_status("Glossary export complete");
    stderr_detail_kv("Entries", terms.len());
    stderr_detail_kv("Path", export_path.display());
    Ok(())
}

#[derive(Serialize)]
struct GlossaryListOutput {
    path: String,
    count: usize,
    terms: Vec<GlossaryEntryOutput>,
}

#[derive(Serialize)]
struct GlossaryEntryOutput {
    term: String,
    og_term: Option<String>,
    definition: String,
    notes: Option<String>,
}
