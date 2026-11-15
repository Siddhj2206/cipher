//! The Endpoint form Settings › Translation shows.

use cipher_llm::Protocol;
use rune::{El, TextEdit, col, row, text, text_input};

use super::widgets::{btn, chip};
use crate::settings::{EFFORTS, Kind, Settings, Status};
use crate::state::State;
use crate::theme::{Pal, SMALL, UI};

/// A labelled one-line field bound to one of the form's [`TextEdit`]s.
fn field(
    label: &str,
    edit: &TextEdit,
    key: &str,
    placeholder: &str,
    bind: fn(&mut Settings) -> &mut TextEdit,
    p: &Pal,
) -> El {
    col()
        .w_full()
        .gap(4.0)
        .child(text(label.to_string()).font(SMALL).color(p.ink2))
        .child(
            text_input(edit, UI)
                .key(key.to_string())
                .w_full()
                .px(10.0)
                .py(7.0)
                .radius(6.0)
                .border(1.0, p.rule)
                .fill(p.paper)
                .single_line()
                .color(p.ink)
                .caret(p.red, p.highlight)
                .placeholder(placeholder.to_string(), p.ink4)
                .on_edit(move |s: &mut State| {
                    let form = s.settings.as_mut().expect("settings are open while edited");
                    form.status = Status::Idle;
                    bind(form)
                }),
        )
}

fn chips<T: Copy + PartialEq + 'static>(
    options: &[(T, &str)],
    on: T,
    key: &str,
    set: fn(&mut Settings, T),
    p: &Pal,
) -> El {
    let mut r = row().gap(6.0);
    for (i, &(value, label)) in options.iter().enumerate() {
        r = r.child(
            chip(label, value == on, p)
                .key(format!("{key}{i}"))
                .on_press(move |s: &mut State| {
                    if let Some(form) = &mut s.settings {
                        set(form, value);
                        form.status = Status::Idle;
                    }
                }),
        );
    }
    r
}

/// The models the signed-in plan offers, as chips: one click makes a model the default.
fn available_models(f: &Settings, p: &Pal) -> El {
    if f.available.is_empty() {
        return col();
    }
    let current = f.translate.text.trim().to_string();
    let mut r = row().gap(6.0).flex_wrap();
    for (i, m) in f.available.iter().enumerate() {
        let m2 = m.clone();
        r = r.child(
            chip(m, *m == current, p)
                .key(format!("ep-avail{i}"))
                .help("Use this model by default")
                .on_press(move |s: &mut State| {
                    if let Some(form) = &mut s.settings {
                        form.translate = rune::TextEdit::new(&m2);
                        form.status = Status::Idle;
                    }
                }),
        );
    }
    col()
        .gap(6.0)
        .child(text("Models on your plan").font(SMALL).color(p.ink2))
        .child(r)
}

/// The Endpoint: where Cipher sends text to translate. Test tries it; Save stores the key in
/// the keychain and the rest in `endpoints.json`, and restarts the worker on it.
pub fn endpoint_form(f: &Settings, p: &Pal) -> El {
    let mut body = col().key("settings").w_full().gap(14.0).child(chips(
        &[
            (Kind::ApiKey, "API key"),
            (Kind::ChatGpt, "Sign in with ChatGPT"),
        ],
        f.kind,
        "kind",
        |f, k| f.kind = k,
        p,
    ));

    match f.kind {
        Kind::ApiKey => {
            body = body
                .child(field(
                    "Base URL",
                    &f.base_url,
                    "ep-url",
                    "https://api.openai.com/v1",
                    |f| &mut f.base_url,
                    p,
                ))
                .child(field(
                    "API key",
                    &f.key,
                    "ep-key",
                    if f.has_key {
                        "Saved in the keychain. Type to replace it."
                    } else {
                        "Kept in the OS keychain, never in a Book"
                    },
                    |f| &mut f.key,
                    p,
                ))
                .child(
                    col()
                        .gap(4.0)
                        .child(text("Protocol").font(SMALL).color(p.ink2))
                        .child(chips(
                            &[
                                (Protocol::ChatCompletions, "Chat Completions"),
                                (Protocol::Responses, "Responses"),
                            ],
                            f.protocol,
                            "proto",
                            |f, v| f.protocol = v,
                            p,
                        )),
                );
        }
        Kind::ChatGpt => {
            let (line, label) = match &f.signed_in {
                Some(account) => (format!("Signed in as {account}"), "Sign in again"),
                None => (
                    "Uses your ChatGPT plan, Free included, and its Codex limits. Cipher opens the browser to sign in.".to_string(),
                    "Sign in with ChatGPT",
                ),
            };
            body = body.child(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(text(line).font(UI).color(p.ink2).grow(1.0).wrap())
                    .child(
                        btn(label, f.signed_in.is_none(), p)
                            .key("ep-signin")
                            .on_press(|s: &mut State| s.sign_in()),
                    ),
            );
        }
    }

    body = body
        .child(
            row()
                .w_full()
                .gap(12.0)
                .child(
                    field(
                        "Default model",
                        &f.translate,
                        "ep-model",
                        "model id",
                        |f| &mut f.translate,
                        p,
                    )
                    .w(0.0)
                    .grow(1.0),
                )
                .child(
                    field(
                        "Small model (optional)",
                        &f.small,
                        "ep-small",
                        "for repairs and notes",
                        |f| &mut f.small,
                        p,
                    )
                    .w(0.0)
                    .grow(1.0),
                ),
        )
        .child(available_models(f, p))
        .child(field(
            "More models Books can choose (comma-separated)",
            &f.others,
            "ep-others",
            "model-a, model-b",
            |f| &mut f.others,
            p,
        ))
        .child(
            col()
                .gap(4.0)
                .child(text("Reasoning effort").font(SMALL).color(p.ink2))
                .child(chips(&EFFORTS, f.effort, "effort", |f, v| f.effort = v, p)),
        )
        .child(field(
            "Name (optional)",
            &f.name,
            "ep-name",
            "shown in the app",
            |f| &mut f.name,
            p,
        ));

    let (status, color) = match &f.status {
        Status::Idle => (String::new(), p.ink3),
        Status::Busy(m) => (m.clone(), p.ink3),
        Status::Ok(m) => (m.clone(), p.green),
        Status::Failed(m) => (m.clone(), p.red),
    };
    body = body.child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(
                text(status)
                    .key("ep-status")
                    .font(SMALL)
                    .color(color)
                    .grow(1.0)
                    .wrap(),
            )
            .child(
                btn("Revert", false, p)
                    .key("ep-cancel")
                    .help("Throw away what was typed here")
                    .on_press(|s: &mut State| s.reload_endpoint()),
            )
            .child(
                btn("Test", false, p)
                    .key("ep-test")
                    .on_press(|s: &mut State| s.test_settings()),
            )
            .child(
                btn("Save", true, p)
                    .key("ep-save")
                    .on_press(|s: &mut State| s.save_settings()),
            ),
    );
    body
}
