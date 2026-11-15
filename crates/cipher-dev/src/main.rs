//! Developer CLI: exercises the Book and the engine before the UI exists. Not shipped.
//! See `docs/design/architecture.md` › cipher-dev.

use std::path::Path;
use std::process::ExitCode;

use cipher_book::{Book, ChapterState};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const USAGE: &str = "usage:
  cipher-dev import <folder> <book.cipher>   import raw/, tl/, glossary.json and style.md
  cipher-dev chapters <book.cipher>          list Chapters and their state
  cipher-dev jobs <book.cipher>              list Jobs
  cipher-dev suggestions <book.cipher>       list Suggestions with their Proposed Edits
  cipher-dev realign <book.cipher>           re-pair Chapters an old import paired by position
  cipher-dev reach <book.cipher> <n> <text>  what a Suggestion on <text> in chapter <n> would reach
  cipher-dev signin                          Sign in with ChatGPT (stored in the OS keychain)
  cipher-dev translate <book.cipher> <n>...  translate Chapters by number, replacing what is there
      Endpoint from the environment: CIPHER_BASE_URL, CIPHER_MODEL, optional CIPHER_SMALL_MODEL,
      CIPHER_API_KEY and CIPHER_EFFORT (minimal|low|medium|high); CIPHER_PROTOCOL=chatgpt|responses;
      with chatgpt, CIPHER_ENDPOINT_ID uses the app's sign-in for that Endpoint";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["import", folder, book] => import(Path::new(folder), Path::new(book)),
        ["chapters", book] => chapters(Path::new(book)),
        ["jobs", book] => jobs(Path::new(book)),
        ["suggestions", book] => suggestions(Path::new(book)),
        ["realign", book] => realign(Path::new(book)),
        ["signin"] => signin(),
        ["reach", book, n, text] => reach(Path::new(book), n, text),
        ["translate", book, numbers @ ..] if !numbers.is_empty() => {
            translate(Path::new(book), numbers)
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Imports a folder laid out like `Test-Books/<name>` (see `cipher_engine::import`).
fn import(folder: &Path, out: &Path) -> Result<()> {
    let r = cipher_engine::import(folder, out)?;
    if r.skipped_terms > 0 {
        eprintln!(
            "skipped {} glossary entries without a source form (og_term)",
            r.skipped_terms
        );
    }
    println!(
        "{}: {} chapters ({} with a translation), {} terms, {} style rules",
        out.display(),
        r.chapters,
        r.translated,
        r.terms,
        r.rules
    );
    Ok(())
}

/// The Endpoint `translate` uses, from the environment. `CIPHER_PROTOCOL=chatgpt` uses the
/// sign-in stored by `cipher-dev signin` in the OS keychain; otherwise `CIPHER_BASE_URL`,
/// `CIPHER_MODEL` and an optional `CIPHER_API_KEY` (kept in memory only).
fn dev_endpoint() -> Result<(
    cipher_llm::Endpoint,
    std::sync::Arc<dyn cipher_llm::Secrets>,
)> {
    use cipher_llm::{Effort, Endpoint, InMemory, Keychain, Protocol, Secrets};
    let var = |name: &str| std::env::var(name).map_err(|_| format!("set {name}"));
    let reasoning = match std::env::var("CIPHER_EFFORT").as_deref() {
        Ok("minimal") | Ok("off") => Effort::Minimal,
        Ok("low") => Effort::Low,
        Ok("medium") => Effort::Medium,
        Ok("high") => Effort::High,
        _ => Effort::ProviderDefault,
    };
    let protocol = match std::env::var("CIPHER_PROTOCOL").as_deref() {
        Ok("chatgpt") => Protocol::ChatGpt,
        Ok("responses") => Protocol::Responses,
        _ => Protocol::ChatCompletions,
    };
    let chatgpt = protocol == Protocol::ChatGpt;
    let endpoint = Endpoint {
        id: if chatgpt {
            // CIPHER_ENDPOINT_ID reuses a sign-in made in the app (its id in endpoints.json).
            std::env::var("CIPHER_ENDPOINT_ID").unwrap_or_else(|_| CHATGPT_DEV_ENDPOINT.into())
        } else {
            "dev".into()
        },
        name: "dev".into(),
        protocol,
        base_url: if chatgpt {
            std::env::var("CIPHER_BASE_URL").unwrap_or_default()
        } else {
            var("CIPHER_BASE_URL")?
        },
        translate_model: var("CIPHER_MODEL")?,
        small_model: std::env::var("CIPHER_SMALL_MODEL").ok(),
        reasoning,
        models: Vec::new(),
    };
    let secrets: std::sync::Arc<dyn Secrets> = if chatgpt {
        std::sync::Arc::new(Keychain)
    } else {
        let mem = InMemory::new();
        if let Ok(key) = std::env::var("CIPHER_API_KEY") {
            mem.set(&endpoint.api_key_id(), &key)?;
        }
        std::sync::Arc::new(mem)
    };
    Ok((endpoint, secrets))
}

const CHATGPT_DEV_ENDPOINT: &str = "dev-chatgpt";

/// Signs in with ChatGPT and stores the Credentials in the OS keychain.
fn signin() -> Result<()> {
    use cipher_llm::{ChatGptSession, Issuer, Keychain, SignIn};
    let keychain = Keychain;
    let previous = ChatGptSession::stored(&keychain, CHATGPT_DEV_ENDPOINT)?;
    let email = previous.as_ref().and_then(|p| p.email.as_deref());
    let sign_in = SignIn::start(Issuer::openai(), email)?;
    println!("Open this URL to sign in:\n\n{}\n", sign_in.url());
    let _ = std::process::Command::new("xdg-open")
        .arg(sign_in.url())
        .spawn();
    let creds = sign_in.finish(std::time::Duration::from_secs(300))?;
    ChatGptSession::save(&keychain, CHATGPT_DEV_ENDPOINT, &creds)?;
    println!(
        "Signed in as {} (account {})",
        creds.describe(),
        creds.account
    );
    Ok(())
}

fn chapters(path: &Path) -> Result<()> {
    let book = Book::open(path)?;
    for c in book.chapters()? {
        let state = match c.state {
            ChapterState::Waiting => "waiting",
            ChapterState::Queued => "queued",
            ChapterState::Translating => "translating",
            ChapterState::Translated => "translated",
            ChapterState::Failed => "failed",
            ChapterState::Empty => "empty",
        };
        println!(
            "{:>6} {:>5} {:<11} {:>4} blocks  {}",
            c.id.0,
            c.number.map(|n| n.to_string()).unwrap_or_default(),
            state,
            c.blocks,
            c.first_line.chars().take(40).collect::<String>()
        );
    }
    Ok(())
}

fn translate(path: &Path, numbers: &[&str]) -> Result<()> {
    use cipher_engine::{Event, Models, Worker};
    let (endpoint, secrets) = dev_endpoint()?;
    let connection = cipher_llm::connect(&endpoint, secrets)?;
    let models = Models {
        translate: connection.translate,
        small: Some(connection.small),
        reasoning: endpoint.reasoning,
        name: format!("{} · {}", endpoint.name, endpoint.translate_model),
    };

    let mut book = Book::open(path)?;
    let chapters = book.chapters()?;
    let mut ids = Vec::new();
    for n in numbers {
        let n: i64 = n.parse()?;
        let c = chapters
            .iter()
            .find(|c| c.number == Some(n))
            .ok_or_else(|| format!("no chapter {n}"))?;
        ids.push(c.id);
    }
    cipher_engine::translate(&mut book, &ids)?;

    let mut worker = Worker::new(book, models)?;
    let started = std::time::Instant::now();
    let mut last_progress = 0;
    let mut emit = |e: Event| match e {
        Event::Progress { done, total, .. } => {
            if done >= last_progress + 20 || done == total {
                last_progress = done;
                eprintln!("  {done}/{total}");
            }
        }
        // The page's view of the stream; the progress line above is enough here.
        Event::Streamed { .. } => {}
        Event::JobStarted { job, kind, chapter } => {
            last_progress = 0;
            eprintln!(
                "job {} {} chapter {:?}",
                job.0,
                kind.as_str(),
                chapter.map(|c| c.0)
            );
        }
        Event::JobFinished { usage, .. } => eprintln!(
            "  done in {:.0?}: {} tokens in, {} out (reasoning {:?}, cached {:?})",
            started.elapsed(),
            usage.input,
            usage.output,
            usage.reasoning,
            usage.cached
        ),
        other => eprintln!("  {other:?}"),
    };
    while worker.step(&mut emit)? {}
    Ok(())
}

fn reach(path: &Path, n: &str, text: &str) -> Result<()> {
    let book = Book::open(path)?;
    let n: i64 = n.parse()?;
    let summary = book
        .chapters()?
        .into_iter()
        .find(|c| c.number == Some(n))
        .ok_or_else(|| format!("no chapter {n}"))?;
    let chapter = book.chapter(summary.id)?;
    let block = chapter
        .translation
        .iter()
        .find(|b| b.text.contains(text))
        .ok_or_else(|| format!("\"{text}\" is not in chapter {n}"))?;
    let selection = cipher_engine::Selection {
        chapter: chapter.id,
        block: block.id,
        text: text.into(),
    };
    for scope in [cipher_book::Scope::Here, cipher_book::Scope::Everywhere] {
        let started = std::time::Instant::now();
        let r = cipher_engine::reach(&book, &selection, scope)?;
        println!(
            "{scope:?}: {} blocks in {} chapters ({} exact), source {:?}, term {:?}, in {:.0?}",
            r.blocks,
            r.chapters,
            r.exact,
            r.source_expr,
            r.term,
            started.elapsed()
        );
    }
    Ok(())
}

fn realign(path: &Path) -> Result<()> {
    let t = std::time::Instant::now();
    let n = cipher_engine::realign_imported(&mut Book::open(path)?)?;
    println!("re-paired {n} chapters in {:?}", t.elapsed());
    Ok(())
}

fn suggestions(path: &Path) -> Result<()> {
    let book = Book::open(path)?;
    for s in book.suggestions(cipher_book::SuggestionFilter::default())? {
        println!(
            "#{} {:?} {:?} quote={:?} source_expr={:?}\n  said: {}",
            s.id.0, s.scope, s.state, s.quote, s.source_expr, s.said
        );
        for e in &s.edits {
            println!("  - block {} {:?} {:?}", e.block.0, e.origin, e.state);
            println!("    before: {}", e.before);
            match (&e.after, &e.skip_reason) {
                (Some(a), _) => println!("    after:  {a}"),
                (None, r) => println!("    skip:   {}", r.as_deref().unwrap_or("")),
            }
        }
    }
    Ok(())
}

fn jobs(path: &Path) -> Result<()> {
    let book = Book::open(path)?;
    for j in book.jobs(false)? {
        println!(
            "{:>5} {:<14} {:<9} chapter {:<5} attempts {} tokens {}/{} {} {}",
            j.id.0,
            j.kind.as_str(),
            j.state.as_str(),
            j.chapter.map(|c| c.0.to_string()).unwrap_or("-".into()),
            j.attempts,
            j.tokens_in.unwrap_or(0),
            j.tokens_out.unwrap_or(0),
            j.model.unwrap_or_default(),
            j.error.unwrap_or_default()
        );
    }
    Ok(())
}
