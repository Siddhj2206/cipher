//! Named states for headless snapshots (`--scene NAME`). A headless run works on a copy of the
//! Book, so scenes that write (a Suggestion, a seeded Review) never touch the real one.

use cipher_book::{
    BlockId, Change, ChapterId, EditOrigin, NewProposedEdit, NewSuggestion, Scope, SuggestionId,
    SuggestionKind, SuggestionState,
};
use rune::TextEdit;

use crate::fix::work::Progress;
use crate::settings::{Kind, Status};
use crate::state::{Screen, State, Tab};

/// A Term used near the top of this Chapter, for the scenes that show a Term card.
fn open_a_term(s: &mut State) {
    let present = &s.lib.stats(s.lib.current()).present;
    if let Some(&t) = present.get(1).or(present.first()) {
        s.show_term(None, t);
    }
}

/// Chapter 227 holds the Terms the prototype's fixes were drawn with.
fn at_227(s: &mut State) {
    if let Some(i) = s.lib.chapters.iter().position(|c| c.number == 227) {
        s.open_chapter(i);
    }
}

fn block_with(s: &State, needle: &str) -> BlockId {
    s.lib
        .page()
        .translation
        .iter()
        .find(|b| b.text.contains(needle))
        .map(|b| b.id)
        .expect("the scene's Book has this text")
}

fn pick(s: &mut State, needle: &str) {
    let id = block_with(s, needle);
    s.select(id, needle.to_string());
}

/// Sends a fix as the fix box would.
fn send(s: &mut State, needle: &str, said: &str, scope: usize, rename: bool) {
    pick(s, needle);
    s.choose_scope(scope);
    s.fix.rename = rename;
    s.open_composer(said);
    s.submit();
}

/// A model-written Suggestion with Proposed Edits seeded directly: `ready` edits that replace
/// `needle` with `to`, `skipped` the model left alone, and `stale` that no longer match.
fn seed_model(
    s: &mut State,
    needle: &str,
    to: &str,
    (ready, skipped, stale): (usize, usize, usize),
) -> SuggestionId {
    let anchor = block_with(s, needle);
    let here = s.lib.chapters[s.lib.current()].id;
    let book = s.lib.book_mut();
    let ids: Vec<ChapterId> = book
        .chapters()
        .expect("chapters")
        .into_iter()
        .map(|c| c.id)
        .collect();
    let mut found = Vec::new();
    for id in ids {
        for b in book.chapter(id).expect("chapter").translation {
            if b.text.contains(needle) {
                found.push(b);
            }
        }
    }
    let make = |b: &cipher_book::Block, after: Option<String>, stale: bool| NewProposedEdit {
        block: b.id,
        before: if stale {
            format!("{} (an earlier wording)", b.text)
        } else {
            b.text.clone()
        },
        after,
        origin: EditOrigin::Model,
        skip_reason: None,
    };
    let mut edits = Vec::new();
    for (i, b) in found.iter().enumerate() {
        let after = Some(b.text.replacen(needle, to, 1));
        if i < ready {
            edits.push(make(b, after, false));
        } else if i < ready + skipped {
            edits.push(NewProposedEdit {
                skip_reason: Some("The English already reads naturally here.".into()),
                ..make(b, None, false)
            });
        } else if i < ready + skipped + stale {
            edits.push(make(b, after, true));
        }
    }
    let applied = book
        .apply(Change::AddSuggestion(NewSuggestion {
            chapter: here,
            block: anchor,
            quote: needle.to_string(),
            said: to.to_string(),
            scope: Scope::Everywhere,
            is_rename: false,
            source_expr: None,
            state: SuggestionState::Ready,
            kind: SuggestionKind::Reader,
            edits: Vec::new(),
        }))
        .expect("suggestion");
    let id = SuggestionId(applied.created[0]);
    book.apply(Change::AddProposedEdits {
        suggestion: id,
        edits,
    })
    .expect("edits");
    s.lib.sync().expect("sync");
    s.tab = Tab::Suggestions;
    id
}

fn review_inline(s: &mut State) {
    at_227(s);
    let id = seed_model(s, "Holy Scripture", "Divine Scripture", (14, 2, 0));
    s.begin_review(id);
}

fn renames(s: &mut State) {
    at_227(s);
    send(s, "Holy Scripture", "Divine Scripture", 1, true);
    send(s, "Magic Corps", "Mage Corps", 0, true);
}

fn compose(s: &mut State) {
    at_227(s);
    pick(s, "Magic Tower Lord");
    s.open_composer("Tower Master");
}

fn requests(s: &mut State) {
    at_227(s);
    send(s, "Holy Scripture", "Divine Scripture", 1, true);
    // Model work with no worker running: it stays queued.
    send(s, "Moriphas", "Make it less formal", 1, false);
    seed_model(s, "Gehennid", "Gehenid", (3, 1, 1));
    send(s, "Magic Corps", "Mage Corps", 0, true);
    let done = s.lib.suggestions[0].id;
    s.accept_all(done);
    s.toast = None;
    s.toast_undo = None;
    s.tab = Tab::Suggestions;
}

fn focus_notes(s: &mut State) {
    s.prefs.contents_open = false;
    s.prefs.context_open = false;
    at_227(s);
    send(s, "Magic Corps", "Mage Corps", 0, true);
    seed_model(s, "Holy Scripture", "Divine Scripture", (4, 0, 0));
}

/// What a reader would copy from a web novel site: a heading, then paragraphs.
const PASTED: &str = "321화\n\n그날 밤, 탑 위에서 바람이 불었다.\n\n테실리드는 조용히 고개를 들었다.\n\n\"이제 시작이군.\"\n\n마탑주의 목소리가 멀리서 들려왔다.";

/// Adds the pasted Chapter (queued) and opens it.
fn add_pasted(s: &mut State) -> usize {
    let intake = s.lib.intake(PASTED).expect("the pasted text is a Chapter");
    let at = s.lib.add_chapter(&intake).expect("the Chapter is added");
    s.open_chapter(at);
    at
}

/// Adds the pasted Chapter and has a worker take its Job, then marks it as `state`.
fn pasted_job(s: &mut State, state: cipher_book::JobState, error: Option<&str>) -> usize {
    let at = add_pasted(s);
    let book = s.lib.book_mut();
    let mut job = book.claim_next_job().expect("claim").expect("a queued Job");
    job.state = state;
    job.error = error.map(String::from);
    book.save_job(&job).expect("save");
    s.lib.refresh_chapters(&[at]).expect("refresh");
    at
}

/// A Pair of Chapter 227 that Repair could not fix.
fn flag_a_pair(s: &mut State) {
    at_227(s);
    let id = block_with(s, "belated regret");
    let pair = s
        .lib
        .page()
        .translation
        .iter()
        .find(|b| b.id == id)
        .and_then(|b| b.pair)
        .expect("the paragraph has a Pair");
    s.lib
        .book_mut()
        .apply(Change::FlagPair {
            pair,
            reason: Some("Korean left in the translation after repair: 회한".into()),
        })
        .expect("flag");
    let at = s.lib.current();
    s.lib.refresh_chapters(&[at]).expect("refresh");
}

/// Two made-up Endpoints: an API-key one with three models, and a ChatGPT plan.
fn endpoints() -> Vec<cipher_llm::Endpoint> {
    use cipher_llm::{Effort, Endpoint, Protocol};
    vec![
        Endpoint {
            id: "e-api".into(),
            name: "api.example.com".into(),
            protocol: Protocol::ChatCompletions,
            base_url: "https://api.example.com/v1".into(),
            translate_model: "model-large".into(),
            small_model: Some("model-small".into()),
            reasoning: Effort::ProviderDefault,
            models: vec!["model-medium".into(), "model-fast".into()],
        },
        Endpoint {
            id: "e-plan".into(),
            name: "ChatGPT".into(),
            protocol: Protocol::ChatGpt,
            base_url: String::new(),
            translate_model: "model-plan".into(),
            small_model: None,
            reasoning: Effort::Low,
            models: Vec::new(),
        },
    ]
}

fn settings(s: &mut State) {
    s.open_settings();
    let form = s.settings.as_mut().expect("open");
    form.all = endpoints();
    form.default = Some("e-api".into());
    form.id = "e-api".into();
    form.is_new = false;
    form.base_url = TextEdit::new("https://api.example.com/v1");
    form.name = TextEdit::new("api.example.com");
    form.translate = TextEdit::new("model-large");
    form.others = TextEdit::new("model-medium, model-fast");
    form.small = TextEdit::new("model-small");
    form.has_key = true;
    form.status = Status::Ok("model-large replied “OK” in 1.2 s · 31 tokens in, 2 out".into());
}

fn settings_chatgpt(s: &mut State) {
    settings(s);
    let f = s.settings.as_mut().expect("open");
    f.id = "e-plan".into();
    f.kind = Kind::ChatGpt;
    f.name = TextEdit::new("ChatGPT");
    f.base_url = TextEdit::new("");
    f.translate = TextEdit::new("gpt-5.1-codex");
    f.small = TextEdit::new("");
    f.others = TextEdit::new("");
    f.has_key = false;
    f.all[1].translate_model = "gpt-5.1-codex".into();
    f.status = Status::Idle;
}

fn settings_no_key(s: &mut State) {
    settings(s);
    let f = s.settings.as_mut().expect("open");
    f.has_key = false;
    f.name = TextEdit::new("Local server");
    f.base_url = TextEdit::new("http://localhost:11434/v1");
    f.status = Status::Idle;
}

fn settings_error(s: &mut State) {
    settings(s);
    s.settings.as_mut().expect("open").status = Status::Failed(
        "Could not test this Endpoint: the server refused the request. Check the base URL, model and credentials, then try again.".into());
}

/// The Shelf with the scene's Book and two more: one new and empty, one renamed.
fn shelf(s: &mut State) {
    s.show_shelf();
    let h = s.home.as_mut().expect("the Shelf");
    let first = h.shelf.books[0].path.clone();
    h.shelf.opened(&first).expect("recents");
    for (title, lang) in [
        ("검은 탑의 기사", "ko"),
        ("The Tower Diaries", "ja"),
        ("The Archmage Returns After a Thousand Years", "ko"),
        ("月下の書庫", "ja"),
        ("Return of the Mount Hua Sect", "zh"),
    ] {
        h.shelf.create(title, lang).expect("create");
    }
}

type Scene = (&'static str, fn(&mut State));

fn history(s: &mut State, conflict: bool, undone: bool) {
    let chapter = s.lib.chapters[0].id;
    let old = s.lib.book().chapter(chapter).expect("chapter");
    let pairs = old
        .source
        .iter()
        .enumerate()
        .map(|(i, b)| cipher_book::NewPair {
            source: vec![b.id],
            translation: vec![
                old.translation
                    .get(i)
                    .map(|b| b.text.clone())
                    .unwrap_or_else(|| "The tower was quiet.".into())
                    + " Revised.",
            ],
            flagged: None,
        })
        .collect();
    let revision = s
        .lib
        .book_mut()
        .apply(Change::SetTranslation { chapter, pairs })
        .expect("translation")
        .revision;
    let block = s.lib.book().chapter(chapter).expect("chapter").translation[0].clone();
    let suggestion = s
        .lib
        .book_mut()
        .apply(Change::AddSuggestion(NewSuggestion {
            chapter,
            block: block.id,
            quote: block.text.clone(),
            said: "Make the opening clearer".into(),
            scope: Scope::Here,
            is_rename: false,
            source_expr: None,
            state: SuggestionState::Ready,
            kind: SuggestionKind::Reader,
            edits: vec![NewProposedEdit {
                block: block.id,
                before: block.text.clone(),
                after: Some(block.text.clone() + " Reviewed."),
                origin: EditOrigin::Model,
                skip_reason: None,
            }],
        }))
        .expect("suggestion");
    let edit = s
        .lib
        .book()
        .suggestions(Default::default())
        .expect("suggestions")
        .into_iter()
        .find(|r| r.id.0 == suggestion.created[0])
        .expect("suggestion")
        .edits[0]
        .id;
    let review = s
        .lib
        .book_mut()
        .apply(Change::DecideEdits {
            decisions: vec![(edit, true)],
        })
        .expect("review")
        .revision;
    s.open_history();
    let selected = if conflict { revision } else { review };
    if let Some(index) = s.history.revisions.iter().position(|r| r.id == selected) {
        s.select_history(index);
    }
    if undone {
        s.undo_history();
    }
}

pub const SCENES: &[Scene] = &[
    ("history-metadata", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Light;
        let term = s.lib.book().glossary().expect("glossary").terms[0].clone();
        s.lib
            .book_mut()
            .apply(Change::AddStoryNote {
                chapter: None,
                text: format!("{} watches the tower.", term.fields.rendering),
            })
            .expect("story note");
        let at = s
            .lib
            .terms
            .iter()
            .position(|t| t.id == term.id)
            .expect("term");
        let sid = s
            .lib
            .use_wording(at, "Tower Keeper", &[])
            .expect("keep")
            .expect("suggestion");
        let decisions = s
            .lib
            .suggestion(sid)
            .expect("suggestion")
            .metadata
            .iter()
            .map(|e| (e.id, true))
            .collect();
        s.review_metadata(sid, decisions);
        s.open_history();
    }),
    ("history", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Light;
        history(s, false, false);
    }),
    ("dark-history", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Dark;
        history(s, false, false);
    }),
    ("history-conflict", |s| history(s, true, false)),
    ("history-undone", |s| history(s, false, true)),
    ("reading", |_| {}),
    ("focus", |s| {
        s.prefs.contents_open = false;
        s.prefs.context_open = false;
        open_a_term(s);
    }),
    ("interlinear", |s| s.interlinear = true),
    ("source", |s| s.source = true),
    ("cast", |s| s.screen = Screen::Cast),
    ("cast-drift", |s| {
        s.screen = Screen::Cast;
        s.cast_open = s.lib.cast_order().first().copied();
    }),
    ("palette", |s| {
        s.palette = Some(TextEdit::new("23"));
    }),
    ("term", |s| {
        s.tab = Tab::Terms;
        open_a_term(s);
    }),
    ("dark", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Dark;
        open_a_term(s);
    }),
    ("dark-cast", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Dark;
        s.screen = Screen::Cast;
    }),
    ("dark-focus", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Dark;
        s.prefs.contents_open = false;
        s.prefs.context_open = false;
        open_a_term(s);
    }),
    ("chip", |s| {
        at_227(s);
        pick(s, "Magic Tower Lord");
    }),
    ("compose", compose),
    ("review", renames),
    ("review-inline", review_inline),
    ("requests", requests),
    ("focus-notes", focus_notes),
    ("dark-review", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Dark;
        review_inline(s);
    }),
    ("dark-compose", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Dark;
        compose(s);
    }),
    ("intake", |s| {
        at_227(s);
        s.open_draft(PASTED);
    }),
    ("dark-intake", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Dark;
        at_227(s);
        s.open_draft(PASTED);
    }),
    ("queued", |s| {
        add_pasted(s);
    }),
    ("translating", |s| {
        let at = pasted_job(s, cipher_book::JobState::Running, None);
        let chapter = s.lib.chapters[at].id;
        s.progress = Some(Progress {
            chapter,
            done: 2,
            total: 4,
        });
        at_227(s);
    }),
    ("failed", |s| {
        pasted_job(
            s,
            cipher_book::JobState::Failed,
            Some("authentication failed: the Endpoint refused the key"),
        );
    }),
    ("flagged", flag_a_pair),
    ("settings", settings),
    ("settings-models", |s| {
        settings(s);
        let form = s.settings.as_mut().expect("open");
        form.available = [
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-4o",
            "gpt-4o-mini",
            "model-fast",
            "model-large",
            "model-medium",
            "model-small",
            "o3",
            "o4-mini",
        ]
        .map(String::from)
        .to_vec();
        form.available_from = form.source();
    }),
    ("settings-models-failed", |s| {
        settings(s);
        let form = s.settings.as_mut().expect("open");
        form.fetch = Status::Failed("The server refused the key. Check it, then retry.".into());
    }),
    ("settings-chatgpt", settings_chatgpt),
    ("settings-no-key", settings_no_key),
    ("settings-error", settings_error),
    ("settings-busy", |s| {
        settings(s);
        s.settings.as_mut().expect("open").status = Status::Busy("Testing…".into());
    }),
    ("settings-chatgpt-busy", |s| {
        settings_chatgpt(s);
        s.settings.as_mut().expect("open").status = Status::Busy("Waiting for the browser…".into());
    }),
    ("settings-chatgpt-signed-in", |s| {
        settings_chatgpt(s);
        let f = s.settings.as_mut().expect("open");
        f.signed_in = Some("reader@example.com".into());
        f.available = vec!["gpt-5.1-codex".into(), "gpt-5.1-codex-mini".into()];
        f.status = Status::Ok("Signed in with ChatGPT.".into());
    }),
    ("settings-general-zoom", |s| {
        s.prefs.zoom = 1.6;
        s.open_settings_at(crate::state::Section::General);
    }),
    ("settings-zoom", |s| {
        settings(s);
        s.prefs.zoom = 1.6;
    }),
    ("settings-chatgpt-zoom", |s| {
        settings_chatgpt(s);
        s.prefs.zoom = 1.6;
    }),
    ("settings-error-zoom", |s| {
        settings_error(s);
        s.prefs.zoom = 1.6;
    }),
    ("settings-about-zoom", |s| {
        s.prefs.zoom = 1.6;
        s.open_settings_at(crate::state::Section::About);
    }),
    ("settings-shortcuts-zoom", |s| {
        s.prefs.zoom = 1.6;
        s.open_settings_at(crate::state::Section::Shortcuts);
    }),
    ("shortcuts-zoom", |s| {
        s.prefs.zoom = 1.6;
        s.shortcuts_open = true;
    }),
    ("home", |_| {}),
    ("shelf", shelf),
    ("dark-shelf", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Dark;
        shelf(s);
    }),
    ("shelf-new", |s| {
        shelf(s);
        s.start_new_book();
        if let Some(f) = &mut s.home.as_mut().expect("shelf").new_book {
            f.title = TextEdit::new("Omniscient Reader");
        }
    }),
    ("shelf-confirm", |s| {
        shelf(s);
        let p = s.home.as_ref().expect("shelf").shelf.books[1].path.clone();
        s.confirm_remove(p);
    }),
    ("shelf-rename", |s| {
        shelf(s);
        let b = s.home.as_ref().expect("shelf").shelf.books[1].clone();
        s.start_rename(b.path, &b.title);
    }),
    ("empty-book", |s| {
        shelf(s);
        let p = s.home.as_ref().expect("shelf").shelf.books[1].path.clone();
        let lib = crate::library::Library::open(&p).expect("open");
        s.show_book(lib);
    }),
    ("dark-settings", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Dark;
        settings(s);
    }),
    ("settings-general", |s| {
        s.open_settings_at(crate::state::Section::General)
    }),
    ("settings-reading", |s| {
        s.open_settings_at(crate::state::Section::Reading);
        s.prefs.reading.size = 20.0;
    }),
    ("dark-settings-reading", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Dark;
        s.open_settings_at(crate::state::Section::Reading);
    }),
    ("settings-shortcuts", |s| {
        s.open_settings_at(crate::state::Section::Shortcuts)
    }),
    ("settings-about", |s| {
        s.open_settings_at(crate::state::Section::About)
    }),
    ("shortcuts", |s| s.shortcuts_open = true),
    ("light-settings-general", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Light;
        s.open_settings_at(crate::state::Section::General);
    }),
    ("reading-sans", |s| {
        s.prefs.reading.typeface = crate::prefs::Typeface::Sans;
        s.prefs.reading.width = 760.0;
        s.prefs.reading.size = 19.0;
    }),
    ("type-popover", |s| s.type_open = true),
    ("light-type-popover", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Light;
        s.type_open = true;
    }),
    ("chapter-menu", |s| {
        let i = s.lib.current() + 2;
        s.chapter_menu = Some((i, 160.0, 470.0));
    }),
    ("model-menu", |s| {
        s.bg.status = crate::fix::work::Status::Running("api.example.com · model-medium".into());
        let _ = s.lib.set_model_choice(Some(&cipher_book::ModelChoice {
            endpoint: "e-api".into(),
            model: "model-medium".into(),
        }));
        s.model_menu = Some((endpoints(), Some("e-api".into())));
    }),
    ("new-chapter", |s| s.open_draft("")),
    ("light-new-chapter-pasted", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Light;
        s.open_draft(PASTED);
    }),
    ("welcome", |s| s.show_welcome()),
    ("light-welcome-key", |s| {
        s.prefs.appearance = crate::prefs::Appearance::Light;
        s.show_welcome();
        s.welcome_pick(Kind::ApiKey);
    }),
    ("welcome-connected", |s| {
        s.show_welcome();
        let form = s.settings.as_mut().expect("open");
        form.all = endpoints();
        form.default = Some("e-api".into());
        if let Some(f) = &mut s.home.as_mut().expect("shelf").new_book {
            f.title = TextEdit::new("전지적 독자 시점");
        }
    }),
];
