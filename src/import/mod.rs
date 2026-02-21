use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use epub::doc::EpubDoc;
use htmd::HtmlToMarkdown;

use crate::book::{BookLayout, init_book};

pub struct ImportReport {
    pub book_dir: std::path::PathBuf,
    pub chapters_imported: usize,
}

pub fn import_epub(epub_path: &Path, force: bool) -> Result<ImportReport> {
    if !epub_path.exists() {
        bail!("EPUB file not found: {}", epub_path.display());
    }

    let book_name = epub_path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("Failed to extract book name from epub filename")?;

    let parent_dir = epub_path
        .parent()
        .context("Cannot determine parent directory for EPUB file (is it at filesystem root?)")?;
    let book_dir = parent_dir.join(book_name);

    let layout = BookLayout::discover(&book_dir);

    if layout.paths.raw_dir.exists() {
        let existing_chapters = count_md_files(&layout.paths.raw_dir)?;

        if existing_chapters > 0 && !force {
            bail!(
                "{} already contains {} chapter(s)\nUse --force to re-import (warning: may orphan existing translations)",
                layout.paths.raw_dir.display(),
                existing_chapters
            );
        }

        if force && existing_chapters > 0 {
            println!(
                "Warning: Re-importing will delete {} existing raw chapters",
                existing_chapters
            );
            println!(
                "Translations in {}/ may become orphaned if chapter order changed",
                layout.paths.out_dir.display()
            );
            print!("Continue? [y/N] ");
            use std::io::{self, BufRead, Write};
            io::stdout().flush()?;

            let stdin = io::stdin();
            let line = stdin.lock().lines().next().transpose()?.unwrap_or_default();
            let answer = line.trim().to_lowercase();
            if answer != "y" && answer != "yes" {
                println!("Aborted");
                std::process::exit(0);
            }

            for entry in fs::read_dir(&layout.paths.raw_dir)? {
                let entry = entry?;
                if entry.path().extension().map(|e| e == "md").unwrap_or(false) {
                    fs::remove_file(entry.path())?;
                }
            }
        }
    }

    let _init_report = init_book(&book_dir, None, None, None)
        .with_context(|| format!("Failed to initialize book at {}", book_dir.display()))?;

    let mut doc = EpubDoc::new(epub_path)
        .with_context(|| format!("Failed to open EPUB: {}", epub_path.display()))?;

    let converter = HtmlToMarkdown::new();
    let mut chapter_count = 0;

    let num_chapters = doc.spine.len();

    for idx in 0..num_chapters {
        if !doc.set_current_chapter(idx) {
            continue;
        }

        let Some((content, _mime)) = doc.get_current() else {
            continue;
        };

        let html = match std::str::from_utf8(&content) {
            Ok(s) => s.to_string(),
            Err(_) => {
                eprintln!(
                    "- Warning: Chapter {} contains invalid UTF-8 sequences (some characters may be corrupted)",
                    idx + 1
                );
                String::from_utf8_lossy(&content).into_owned()
            }
        };

        if is_empty_chapter(&html) {
            continue;
        }

        let markdown = converter
            .convert(&html)
            .with_context(|| format!("Failed to convert chapter {} to markdown", idx + 1))?;

        if markdown.trim().is_empty() {
            continue;
        }

        chapter_count += 1;
        let filename = format!("{:03}.md", chapter_count);
        let chapter_path = layout.paths.raw_dir.join(&filename);

        fs::write(&chapter_path, markdown.trim())
            .with_context(|| format!("Failed to write {}", chapter_path.display()))?;
    }

    println!(
        "Imported {} of {} chapters from {}",
        chapter_count,
        num_chapters,
        epub_path.display()
    );
    println!("Book initialized at: {}", book_dir.display());

    Ok(ImportReport {
        book_dir,
        chapters_imported: chapter_count,
    })
}

fn count_md_files(dir: &Path) -> Result<usize> {
    let count = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|ext| ext == "md").unwrap_or(false))
        .count();
    Ok(count)
}

fn is_empty_chapter(html: &str) -> bool {
    let text = html
        .replace("<br/>", " ")
        .replace("<br />", " ")
        .replace("<br>", " ");

    let text = strip_html_tags(&text);

    let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();

    text.len() < 50
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;

    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }

    result
}
