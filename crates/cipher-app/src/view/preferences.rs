//! The Settings window (Ctrl+,): a section list on the left and the section on the right.
//! Everything but the Endpoint applies the moment it changes; the Endpoint has Test and Save,
//! since a half-typed key should never be used. And the keyboard shortcuts sheet (Ctrl+/).

use rune::tok::TextStyle;
use rune::{Cursor, El, col, el, row, text};

use super::sheets::endpoint_form;
use super::widgets::{btn, card, kbd, scrolling, segmented, setting_row, switch};
use crate::keys::{SHORTCUTS, groups};
use crate::prefs::{Appearance, Prefs, Reading, Typeface};
use crate::shelf::Shelf;
use crate::state::{Section, State};
use crate::theme::{LABEL, Pal, SERIF, SMALL, UI, UI_MED, body};

fn scrim(p: &Pal) -> El {
    col()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(p.scrim)
        .items_center()
        .justify_center()
        .padding(40.0)
}

pub fn settings(s: &State, p: &Pal) -> Option<El> {
    let section = s.settings_page?;
    let mut nav = col()
        .w(196.0)
        .h_full()
        .fill(p.side)
        .px(10.0)
        .pt(22.0)
        .gap(2.0)
        .border_sides([0.0, 1.0, 0.0, 0.0], p.rule)
        .child(text("SETTINGS").font(LABEL).color(p.ink3).px(10.0).pb(10.0));
    for sec in Section::ALL {
        let on = sec == section;
        nav = nav.child(
            text(sec.name())
                .key(format!("set-{}", sec.name().to_lowercase()))
                .font(if on { UI_MED } else { UI })
                .color(if on { p.ink } else { p.ink2 })
                .w_full()
                .px(10.0)
                .py(6.0)
                .radius(6.0)
                .fill(if on {
                    p.selected
                } else {
                    rune::Color::TRANSPARENT
                })
                .hover(p.hover)
                .cursor(Cursor::Pointer)
                .on_press(move |s: &mut State| s.open_settings_at(sec)),
        );
    }

    let body = match section {
        Section::General => general(s, p),
        Section::Reading => reading(s, p),
        Section::Translation => match &s.settings {
            Some(f) => col()
                .w_full()
                .child(intro("Endpoints and the models they offer. Each Book picks one of these models from its header and uses the default otherwise. Keys stay in the system keychain, never in a Book.", p))
                .child(endpoint_list(f, p))
                .child(
                    text(if f.is_new { "NEW ENDPOINT".to_string() } else { format!("EDITING {}", f.endpoint().map(|e| e.name.to_uppercase()).unwrap_or_default()) })
                        .font(LABEL)
                        .color(p.ink3)
                        .pt(26.0)
                        .pb(12.0),
                )
                .child(endpoint_form(f, p)),
            None => col(),
        },
        Section::Shortcuts => shortcuts_list(p),
        Section::About => about(s, p),
    };
    let head = row()
        .w_full()
        .center_y()
        .pb(6.0)
        .child(
            text(section.name())
                .font(TextStyle::new(24.0, 400.0).family(SERIF))
                .color(p.ink)
                .grow(1.0),
        )
        .child(
            text("✕")
                .key("set-close")
                .font(UI)
                .color(p.ink3)
                .px(8.0)
                .py(4.0)
                .radius(6.0)
                .hover(p.hover)
                .cursor(Cursor::Pointer)
                .help("Close (Esc)")
                .on_press(|s: &mut State| s.close_settings()),
        );
    let pane = scrolling(
        col()
            .w_full()
            .px(36.0)
            .pt(24.0)
            .pb(36.0)
            .child(head)
            .child(body),
        "settings-pane",
    );
    let window = card(p)
        .key("settings-window")
        .w_full()
        .max_w(920.0)
        .h_full()
        .max_h(800.0)
        .clip_rounded(10.0)
        .modal(true, |s: &mut State| s.close_settings())
        .child(
            // Sections switch in place, with no fade or spring.
            row()
                .w_full()
                .h_full()
                .instant_changes()
                .child(nav)
                .child(col().grow(1.0).min_w(0.0).h_full().child(pane)),
        );
    Some(scrim(p).child(window))
}

/// The saved Endpoints, one row each, and a way to add another.
fn endpoint_list(f: &crate::settings::Settings, p: &Pal) -> El {
    let mut list = col().w_full();
    for (i, e) in f.all.iter().enumerate() {
        let is_default = f.default.as_deref() == Some(e.id.as_str());
        let editing = !f.is_new && f.id() == e.id;
        let models = e.all_models();
        let what = match e.protocol {
            cipher_llm::Protocol::ChatGpt => "ChatGPT plan".to_string(),
            _ => e.base_url.clone(),
        };
        let more = if models.len() > 1 {
            format!(" and {} more", models.len() - 1)
        } else {
            String::new()
        };
        let mut actions = row().gap(6.0).center_y();
        if is_default {
            actions = actions.child(text("Default").font(SMALL).color(p.green).px(6.0));
        } else {
            let id = e.id.clone();
            actions = actions.child(
                btn("Make default", false, p)
                    .key(format!("prov-default{i}"))
                    .on_press(move |s: &mut State| s.make_default_endpoint(&id)),
            );
        }
        if !editing {
            let id = e.id.clone();
            actions = actions.child(
                btn("Edit", false, p)
                    .key(format!("prov-edit{i}"))
                    .on_press(move |s: &mut State| s.edit_endpoint(&id)),
            );
        }
        let id = e.id.clone();
        actions = actions.child(
            btn("Remove", false, p)
                .key(format!("prov-remove{i}"))
                .help("Forget this Endpoint and its key. Books that chose it use the default.")
                .on_press(move |s: &mut State| s.remove_endpoint(&id)),
        );
        list = list.child(
            row()
                .key(format!("prov{i}"))
                .w_full()
                .py(10.0)
                .px(12.0)
                .gap(12.0)
                .center_y()
                .radius(7.0)
                .fill(if editing {
                    p.selected
                } else {
                    rune::Color::TRANSPARENT
                })
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text(e.name.clone()).font(UI_MED).color(p.ink))
                        .child(
                            text(format!("{what} · {}{more}", models[0]))
                                .font(SMALL)
                                .color(p.ink3)
                                .truncate(),
                        ),
                )
                .child(actions),
        );
    }
    if f.all.is_empty() {
        list = list.child(
            text("No Endpoints yet. Add one below.")
                .font(UI)
                .color(p.ink3)
                .py(8.0),
        );
    }
    list.child(
        row().pt(8.0).child(
            btn("+ Add Endpoint", !f.all.is_empty() && !f.is_new, p)
                .key("prov-add")
                .on_press(|s: &mut State| s.add_endpoint()),
        ),
    )
}

fn intro(line: &str, p: &Pal) -> El {
    text(line.to_string())
        .font(SMALL)
        .color(p.ink3)
        .wrap()
        .pb(14.0)
}

/// A reset arrow beside a control whose value isn't the default.
fn with_reset(control: El, changed: bool, key: &str, reset: fn(&mut State), p: &Pal) -> El {
    let mut r = row().gap(8.0).center_y();
    if changed {
        r = r.child(
            text("↺")
                .key(format!("{key}-reset"))
                .font(UI)
                .color(p.ink3)
                .px(4.0)
                .radius(4.0)
                .hover(p.hover)
                .cursor(Cursor::Pointer)
                .help("Back to the default")
                .on_press(move |s: &mut State| reset(s)),
        );
    }
    r.child(control)
}

fn general(s: &State, p: &Pal) -> El {
    let d = Prefs::default();
    let zoom_label = format!("{}%", (s.prefs.zoom * 100.0).round());
    let books = s
        .home
        .as_ref()
        .map(|h| h.shelf.dir.clone())
        .or_else(Shelf::default_dir)
        .map_or_else(|| "—".to_string(), |d| d.display().to_string());
    let rows = col()
        .w_full()
        .child(setting_row(
            "Appearance",
            "Follow the desktop's light or dark, or pick one.",
            with_reset(
                segmented(
                    &[
                        (Appearance::System, "System"),
                        (Appearance::Light, "Light"),
                        (Appearance::Dark, "Dark"),
                    ],
                    s.prefs.appearance,
                    "set-appearance",
                    |s, v| s.prefs.appearance = v,
                    p,
                ),
                s.prefs.appearance != d.appearance,
                "set-appearance",
                |s| s.prefs.appearance = Appearance::System,
                p,
            ),
            p,
        ))
        .child(setting_row(
            "Interface size",
            "Scales everything in the window, on top of the display's own scale.",
            with_reset(
                row()
                    .gap(6.0)
                    .center_y()
                    .child(
                        btn("−", false, p)
                            .key("set-zoom-out")
                            .on_press(|s: &mut State| s.prefs.step_zoom(false)),
                    )
                    .child(
                        text(zoom_label)
                            .font(UI_MED)
                            .color(p.ink)
                            .w(48.0)
                            .align(rune::Align::Center),
                    )
                    .child(
                        btn("+", false, p)
                            .key("set-zoom-in")
                            .on_press(|s: &mut State| s.prefs.step_zoom(true)),
                    ),
                (s.prefs.zoom - 1.0).abs() > 0.01,
                "set-zoom",
                |s| s.prefs.zoom = 1.0,
                p,
            ),
            p,
        ))
        .child(setting_row(
            "Reopen the last Book",
            "Start where you left off instead of on the Shelf.",
            switch(s.prefs.reopen_last, "set-reopen", p)
                .on_press(|s: &mut State| s.prefs.reopen_last = !s.prefs.reopen_last),
            p,
        ))
        .child(setting_row(
            "Books folder",
            &books,
            btn("Show", false, p)
                .key("set-books")
                .on_press(|s: &mut State| s.show_books_folder()),
            p,
        ));
    if let Some(error) = &s.settings_books_error {
        rows.child(
            text(error.clone())
                .key("set-books-error")
                .font(SMALL)
                .color(p.red)
                .wrap_anywhere()
                .pt(8.0),
        )
    } else {
        rows
    }
}

fn reading(s: &State, p: &Pal) -> El {
    let r = s.prefs.reading;
    let d = Reading::default();
    let size_label = format!("{} pt", r.size);
    let preview = col()
        .w_full()
        .mt(20.0)
        .padding(28.0)
        .radius(8.0)
        .fill(p.paper)
        .border(1.0, p.rule)
        .items_center()
        .child(
            text("The rain had not stopped for three days. Ian pulled his coat tighter and looked up at the gate of the Academy, where his name had once meant nothing at all.")
                .key("set-preview")
                .font(body(&r))
                .color(p.ink)
                .wrap()
                .w_full()
                .max_w(r.width),
        );
    col()
        .w_full()
        .child(setting_row(
            "Typeface",
            "",
            with_reset(
                segmented(
                    &[(Typeface::Serif, "Serif"), (Typeface::Sans, "Sans")],
                    r.typeface,
                    "set-face",
                    |s, v| s.prefs.reading.typeface = v,
                    p,
                ),
                r.typeface != d.typeface,
                "set-face",
                |s| s.prefs.reading.typeface = Reading::default().typeface,
                p,
            ),
            p,
        ))
        .child(setting_row(
            "Text size",
            "Ctrl and = or − change it while reading.",
            with_reset(
                row()
                    .gap(6.0)
                    .center_y()
                    .child(
                        btn("−", false, p)
                            .key("set-size-down")
                            .on_press(|s: &mut State| s.prefs.reading.step_size(false)),
                    )
                    .child(
                        text(size_label)
                            .font(UI_MED)
                            .color(p.ink)
                            .w(48.0)
                            .align(rune::Align::Center),
                    )
                    .child(
                        btn("+", false, p)
                            .key("set-size-up")
                            .on_press(|s: &mut State| s.prefs.reading.step_size(true)),
                    ),
                r.size != d.size,
                "set-size",
                |s| s.prefs.reading.size = Reading::default().size,
                p,
            ),
            p,
        ))
        .child(setting_row(
            "Line spacing",
            "",
            with_reset(
                segmented(
                    &Reading::LEADINGS,
                    r.leading,
                    "set-leading",
                    |s, v| s.prefs.reading.leading = v,
                    p,
                ),
                r.leading != d.leading,
                "set-leading",
                |s| s.prefs.reading.leading = Reading::default().leading,
                p,
            ),
            p,
        ))
        .child(setting_row(
            "Column width",
            "",
            with_reset(
                segmented(
                    &Reading::WIDTHS,
                    r.width,
                    "set-width",
                    |s, v| s.prefs.reading.width = v,
                    p,
                ),
                r.width != d.width,
                "set-width",
                |s| s.prefs.reading.width = Reading::default().width,
                p,
            ),
            p,
        ))
        .child(preview)
}

/// Every shortcut, grouped: Settings › Shortcuts and the Ctrl+/ sheet.
fn shortcuts_list(p: &Pal) -> El {
    let mut out = col().w_full().gap(18.0);
    for g in groups() {
        let mut list = col()
            .w_full()
            .child(text(g.to_uppercase()).font(LABEL).color(p.ink3).pb(6.0));
        for (_, keys, what) in SHORTCUTS.iter().filter(|(gg, _, _)| *gg == g) {
            let mut caps = row().gap(4.0);
            for (i, alt) in keys.split(" / ").enumerate() {
                if i > 0 {
                    caps = caps.child(text("or").font(SMALL).color(p.ink4));
                }
                if alt.contains(' ') && !alt.contains('+') {
                    caps = caps.child(text(alt.to_string()).font(SMALL).color(p.ink3));
                    continue;
                }
                for (j, part) in alt.split('+').enumerate() {
                    if j > 0 {
                        caps = caps.child(text("+").font(SMALL).color(p.ink4));
                    }
                    caps = caps.child(kbd(part, p));
                }
            }
            list = list.child(
                row()
                    .w_full()
                    .py(7.0)
                    .center_y()
                    .border_sides([0.0, 0.0, 1.0, 0.0], p.rule)
                    .child(text(what.to_string()).font(UI).color(p.ink2).grow(1.0))
                    .child(caps),
            );
        }
        out = out.child(list);
    }
    out
}

fn about(s: &State, p: &Pal) -> El {
    let config = cipher_llm::Endpoints::default_path()
        .and_then(|p| p.parent().map(|d| d.display().to_string()))
        .unwrap_or_default();
    let _ = s;
    col()
        .w_full()
        .gap(6.0)
        .child(
            text("Cipher")
                .font(TextStyle::new(30.0, 400.0).family(SERIF))
                .color(p.ink),
        )
        .child(
            text(format!("Version {}", env!("CARGO_PKG_VERSION")))
                .font(UI)
                .color(p.ink2),
        )
        .child(
            text("Reads and translates web novels with the language model you choose.")
                .font(UI)
                .color(p.ink3)
                .wrap()
                .pb(12.0),
        )
        .child(setting_row(
            "Settings and the Shelf's memory",
            &config,
            el(),
            p,
        ))
}

/// The keyboard shortcuts sheet (Ctrl+/ or F1).
pub fn shortcuts_sheet(s: &State, p: &Pal) -> Option<El> {
    if !s.shortcuts_open {
        return None;
    }
    let sheet = card(p)
        .key("shortcuts")
        .w(560.0)
        .h_full()
        .max_h(620.0)
        .clip_rounded(10.0)
        .modal(true, |s: &mut State| s.shortcuts_open = false)
        .child(
            row()
                .w_full()
                .px(24.0)
                .pt(20.0)
                .pb(10.0)
                .center_y()
                .child(
                    text("Keyboard shortcuts")
                        .font(TextStyle::new(20.0, 400.0).family(SERIF))
                        .color(p.ink)
                        .grow(1.0),
                )
                .child(
                    row()
                        .key("shortcuts-close")
                        .cursor(Cursor::Pointer)
                        .help("Close (Esc)")
                        .child(kbd("Esc", p))
                        .on_press(|s: &mut State| s.shortcuts_open = false)
                        .autofocus(),
                ),
        )
        .child(scrolling(
            col().w_full().px(24.0).pb(22.0).child(shortcuts_list(p)),
            "shortcuts-list",
        ));
    Some(scrim(p).child(sheet))
}

/// The "Aa" popover under the header: what a reader changes while reading, applied at once.
pub fn type_popover(s: &State, p: &Pal) -> Option<El> {
    if !s.type_open || s.home.is_some() {
        return None;
    }
    let r = s.prefs.reading;
    let line = |label: &str, control: El| {
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(text(label.to_string()).font(SMALL).color(p.ink3).w(64.0))
            .child(el().grow(1.0))
            .child(control)
    };
    let size = row()
        .gap(6.0)
        .center_y()
        .child(
            text("A")
                .key("type-smaller")
                .font(TextStyle::new(13.0, 400.0).family(SERIF))
                .color(p.ink)
                .w(34.0)
                .py(4.0)
                .align(rune::Align::Center)
                .radius(5.0)
                .border(1.0, p.rule)
                .hover(p.hover)
                .cursor(Cursor::Pointer)
                .help("Smaller (Ctrl+-)")
                .on_press(|s: &mut State| s.prefs.reading.step_size(false)),
        )
        .child(
            text(format!("{}", r.size))
                .font(UI_MED)
                .color(p.ink)
                .w(30.0)
                .align(rune::Align::Center),
        )
        .child(
            text("A")
                .key("type-bigger")
                .font(TextStyle::new(19.0, 400.0).family(SERIF))
                .color(p.ink)
                .w(34.0)
                .align(rune::Align::Center)
                .radius(5.0)
                .border(1.0, p.rule)
                .hover(p.hover)
                .cursor(Cursor::Pointer)
                .help("Bigger (Ctrl+=)")
                .on_press(|s: &mut State| s.prefs.reading.step_size(true)),
        );
    let body = col()
        .key("type-popover")
        .w(320.0)
        .padding(14.0)
        .gap(12.0)
        .child(line("Size", size))
        .child(line(
            "Typeface",
            segmented(
                &[(Typeface::Serif, "Serif"), (Typeface::Sans, "Sans")],
                r.typeface,
                "type-face",
                |s, v| s.prefs.reading.typeface = v,
                p,
            ),
        ))
        .child(line(
            "Spacing",
            segmented(
                &Reading::LEADINGS,
                r.leading,
                "type-leading",
                |s, v| s.prefs.reading.leading = v,
                p,
            ),
        ))
        .child(line(
            "Width",
            segmented(
                &Reading::WIDTHS,
                r.width,
                "type-width",
                |s, v| s.prefs.reading.width = v,
                p,
            ),
        ))
        .child(line(
            "Theme",
            segmented(
                &[
                    (Appearance::System, "System"),
                    (Appearance::Light, "Light"),
                    (Appearance::Dark, "Dark"),
                ],
                s.prefs.appearance,
                "type-theme",
                |s, v| s.prefs.appearance = v,
                p,
            ),
        ))
        .child(
            text("More in Settings…")
                .key("type-more")
                .font(SMALL)
                .color(p.ink3)
                .cursor(Cursor::Pointer)
                .on_press(|s: &mut State| {
                    s.type_open = false;
                    s.open_settings_at(Section::Reading)
                }),
        );
    Some(
        card(p)
            .child(body)
            .anchored("h-type", rune::Side::Bottom, rune::Align::End, 6.0)
            .modal(true, |s: &mut State| s.type_open = false),
    )
}
