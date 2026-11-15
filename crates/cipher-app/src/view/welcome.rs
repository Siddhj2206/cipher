//! The welcome: a title page for the first run. Two steps, each done in place: connect a
//! model (the Endpoint form, as in Settings), and start the first Book on its own blank cover.

use rune::tok::TextStyle;
use rune::{Color, Cursor, El, col, el, row, text};

use super::sheets::endpoint_form;
use super::shelf::{blank_cover, new_book_label};
use super::widgets::{card, scrolling};
use crate::home::Home;
use crate::settings::Kind;
use crate::state::State;
use crate::theme::{Pal, SERIF, SMALL, UI, UI_MED};

fn step(n: u32, title: &str, status: Option<El>, body: El, p: &Pal) -> El {
    let mut head = row().w_full().gap(12.0).center_y().child(
        text(title.to_string())
            .font(TextStyle::new(22.0, 400.0).family(SERIF))
            .color(p.ink)
            .grow(1.0),
    );
    if let Some(st) = status {
        head = head.child(st);
    }
    row()
        .key(format!("welcome-step{n}"))
        .w_full()
        .pt(30.0)
        .pb(34.0)
        .gap(8.0)
        .border_sides([1.0, 0.0, 0.0, 0.0], p.rule)
        .child(
            text(n.to_string())
                .font(TextStyle::new(34.0, 300.0).lh(34.0).family(SERIF))
                .color(p.red)
                .w(56.0),
        )
        .child(col().grow(1.0).min_w(0.0).gap(16.0).child(head).child(body))
}

/// A way to connect, as a quiet card to press.
fn way(key: &str, title: &str, about: &str, kind: Kind, p: &Pal) -> El {
    card(p)
        .key(key.to_string())
        .grow(1.0)
        .w(0.0)
        .padding(18.0)
        .gap(6.0)
        .shadow(1.0, 3.0, p.shadow)
        .hover(p.hover)
        .cursor(Cursor::Pointer)
        .child(text(title.to_string()).font(UI_MED).color(p.ink))
        .child(text(about.to_string()).font(SMALL).color(p.ink3).wrap())
        .on_press(move |s: &mut State| s.welcome_pick(kind))
}

fn link(label: &str, key: &str, p: &Pal) -> El {
    text(label.to_string())
        .key(key.to_string())
        .font(UI)
        .color(p.ink2)
        .cursor(Cursor::Pointer)
}

pub fn welcome(s: &State, h: &Home, p: &Pal) -> El {
    let w = s.welcome.as_ref().expect("the welcome is open");

    // 1. A model.
    let (status, connect) = match s.connected() {
        Some(what) => (
            Some(
                text(format!("✓ {what}"))
                    .key("welcome-connected")
                    .font(UI)
                    .color(p.green),
            ),
            text("Connected. Each Book can pick another of your models later, from its header.")
                .font(SMALL)
                .color(p.ink3),
        ),
        None => {
            let body = match (&s.settings, w.picked) {
                (Some(f), true) => col().w_full().child(endpoint_form(f, p)),
                _ => col()
                    .w_full()
                    .gap(12.0)
                    .child(
                        text("Cipher sends each chapter to a language model to translate it. Bring one; your key or sign-in stays in the system keychain.")
                            .font(UI)
                            .color(p.ink2)
                            .wrap(),
                    )
                    .child(
                        row()
                            .w_full()
                            .gap(12.0)
                            .child(way(
                                "welcome-chatgpt",
                                "Sign in with ChatGPT",
                                "Free or paid, with your plan's Codex limits. Cipher opens the browser to sign in.",
                                Kind::ChatGpt,
                                p,
                            ))
                            .child(way(
                                "welcome-key",
                                "Use an API key",
                                "OpenAI, OpenRouter, DeepSeek, a local server: anything OpenAI-compatible.",
                                Kind::ApiKey,
                                p,
                            )),
                    ),
            };
            (None, body)
        }
    };

    // 2. A Book: its blank cover, typed on, as on the Shelf.
    let book = match &h.new_book {
        Some(form) => row()
            .w_full()
            .gap(28.0)
            .child(
                col()
                    .child(blank_cover(h, form, s))
                    .child(new_book_label(form, false, s, p)),
            )
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(10.0)
                    .pt(8.0)
                    .child(
                        text("Type the title on the cover and stamp the language it's written in. Then paste its chapters, one after another, onto the pages that follow.")
                            .font(UI)
                            .color(p.ink2)
                            .wrap(),
                    )
                    .child(el().h(8.0))
                    .child(text("Already have chapters?").font(SMALL).color(p.ink3))
                    .child(
                        link("Import a folder of chapters…", "welcome-import", p)
                            .on_press_tx(|s: &mut State, tx| s.pick_import(tx)),
                    )
                    .child(
                        link("Open a .cipher book…", "welcome-open", p)
                            .on_press_tx(|s: &mut State, tx| s.pick_book_file(tx)),
                    ),
            ),
        None => col(),
    };

    let mut page = col()
        .key("welcome")
        .w(720.0)
        .pt(92.0)
        .pb(80.0)
        .child(
            text("Cipher")
                .font(TextStyle::new(64.0, 400.0).italic().family(SERIF))
                .color(p.ink),
        )
        .child(
            text("Read web novels in your own language.")
                .font(TextStyle::new(22.0, 400.0).family(SERIF))
                .color(p.ink2)
                .pt(4.0),
        )
        .child(
            text("Each chapter is translated by the model you choose, with names, terms and forms of address kept the same from the first chapter to the last. You read; you fix what reads wrong; Cipher remembers.")
                .font(UI)
                .color(p.ink3)
                .wrap()
                .w(560.0)
                .pt(14.0)
                .pb(46.0),
        )
        .child(step(1, "Connect a model", status, connect, p))
        .child(step(2, "Start your first book", None, book, p));
    if let Some(e) = &h.error {
        page = page.child(text(e.clone()).font(UI).color(p.red).wrap().pb(16.0));
    }
    if let Some(busy) = &h.busy {
        page = page.child(text(busy.clone()).font(UI).color(p.ink2).pb(16.0));
    }
    page = page.child(
        row()
            .w_full()
            .pt(20.0)
            .border_sides([1.0, 0.0, 0.0, 0.0], p.rule)
            .child(el().grow(1.0))
            .child(
                link("Skip, go to the Shelf →", "welcome-skip", p).on_press(|s: &mut State| {
                    s.finish_welcome();
                    if let Some(h) = &mut s.home {
                        h.new_book = None;
                    }
                }),
            ),
    );

    col()
        .w_full()
        .h_full()
        .fill(p.paper)
        .radial_gradient(
            [0.5, 0.0],
            [900.0, 700.0],
            [
                (0.0, Color::WHITE.alpha(if s.dark() { 0.03 } else { 0.30 })),
                (1.0, Color::WHITE.alpha(0.0)),
            ],
        )
        .child(row().w_full().h(52.0).window_drag())
        .child(scrolling(
            col().w_full().items_center().child(page),
            "welcome-scroll",
        ))
}
