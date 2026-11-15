//! Small parts shared by the views.

use rune::tok::TextStyle;
use rune::{Color, Cursor, El, Icon, ScrollSpec, col, el, icon, row, text};

use crate::theme::{LABEL, Pal, SMALL, UI};

pub fn kbd(k: &str, p: &Pal) -> El {
    text(k.to_string())
        .font(TextStyle::new(11.0, 500.0))
        .color(p.ink3)
        .px(5.0)
        .py(1.0)
        .radius(4.0)
        .border(1.0, p.rule)
}

pub fn icon_btn(i: &'static Icon, key: &str, on: bool, p: &Pal) -> El {
    el().key(key.to_string())
        .size(28.0, 28.0)
        .center()
        .radius(6.0)
        .hover(p.hover)
        .cursor(Cursor::Pointer)
        .fill(if on { p.selected } else { Color::TRANSPARENT })
        .child(icon(i, 16.0).color(if on { p.ink } else { p.ink3 }))
}

pub fn dot(color: Color, d: f32) -> El {
    el().size(d, d).radius(d / 2.0).fill(color)
}

/// An unfilled dot: a Chapter that is waiting and has not been queued.
pub fn ring(color: Color, d: f32) -> El {
    el().size(d, d).radius(d / 2.0).border(1.5, color)
}

pub fn card(p: &Pal) -> El {
    col()
        .fill(p.card)
        .radius(10.0)
        .border(1.0, p.rule)
        .shadow(10.0, 30.0, p.shadow)
}

/// A tracked uppercase label with a quiet note on the right.
pub fn section(label: &str, extra: String, p: &Pal) -> El {
    row()
        .w_full()
        .px(18.0)
        .pt(18.0)
        .pb(8.0)
        .center_y()
        .child(text(label.to_string()).font(LABEL).color(p.ink3).grow(1.0))
        .child(text(extra).font(SMALL).color(p.ink4))
}

/// A header-bar toggle: quiet text that takes a fill when on.
pub fn toggle(label: &str, on: bool, key: &str, p: &Pal) -> El {
    text(label.to_string())
        .font(UI)
        .color(if on { p.ink } else { p.ink3 })
        .px(8.0)
        .py(4.0)
        .radius(6.0)
        .fill(if on { p.selected } else { Color::TRANSPARENT })
        .hover(p.hover)
        .cursor(Cursor::Pointer)
        .key(key.to_string())
}

pub fn scrolling(body: El, key: &str) -> El {
    body.scroll(ScrollSpec {
        stick_bottom: false,
    })
    .key(key.to_string())
    .grow(1.0)
    .min_h(0.0)
}

pub fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// A long line cut to about 32 characters, for tooltips and result lists.
pub fn short(t: &str) -> String {
    if t.chars().count() > 32 {
        format!("{}…", t.chars().take(30).collect::<String>().trim_end())
    } else {
        t.to_string()
    }
}

/// A button: filled with ink when primary, else an outline.
pub fn btn(label: &str, primary: bool, p: &Pal) -> El {
    let (bg, fg) = if primary {
        (p.ink, p.paper)
    } else {
        (Color::TRANSPARENT, p.ink2)
    };
    row()
        .h(26.0)
        .px(10.0)
        .center_y()
        .radius(5.0)
        .fill(bg)
        .hover(if primary { p.primary_hover } else { p.hover })
        .cursor(Cursor::Pointer)
        .border(1.0, if primary { p.ink } else { p.rule })
        .child(text(label.to_string()).font(crate::theme::UI_MED).color(fg))
}

/// A toolbar button: an icon and a label, bordered; `primary` fills it with ink.
pub fn tool_btn(i: &'static rune::Icon, label: &str, primary: bool, p: &Pal) -> El {
    let (bg, fg, border) = if primary {
        (p.ink, p.paper, p.ink)
    } else {
        (p.card, p.ink, p.rule)
    };
    row()
        .h(30.0)
        .pl(10.0)
        .pr(12.0)
        .gap(7.0)
        .center_y()
        .radius(6.0)
        .fill(bg)
        .border(1.0, border)
        .hover(if primary { p.primary_hover } else { p.hover })
        .cursor(Cursor::Pointer)
        .child(rune::icon(i, 15.0).color(fg))
        .child(text(label.to_string()).font(crate::theme::UI_MED).color(fg))
}

/// A pill choice such as a Scope.
pub fn chip(label: &str, on: bool, p: &Pal) -> El {
    text(label.to_string())
        .font(SMALL)
        .color(if on { p.paper } else { p.ink2 })
        .px(9.0)
        .py(3.0)
        .radius(12.0)
        .fill(if on { p.ink } else { Color::TRANSPARENT })
        .border(1.0, if on { p.ink } else { p.rule })
        .hover(if on { p.ink } else { p.hover })
        .cursor(Cursor::Pointer)
}

/// A segmented control: one of a few choices, the chosen one raised like a key.
pub fn segmented<T: Copy + PartialEq + 'static>(
    options: &[(T, &str)],
    on: T,
    key: &str,
    set: fn(&mut crate::state::State, T),
    p: &Pal,
) -> El {
    let mut r = row()
        .padding(2.0)
        .gap(2.0)
        .radius(7.0)
        .fill(p.side)
        .border(1.0, p.rule);
    for (i, &(value, label)) in options.iter().enumerate() {
        let chosen = value == on;
        let mut seg = text(label.to_string())
            .key(format!("{key}-{i}"))
            .font(SMALL)
            .color(if chosen { p.ink } else { p.ink3 })
            .px(11.0)
            .py(4.0)
            .radius(5.0)
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut crate::state::State| set(s, value));
        seg = if chosen {
            seg.fill(p.card).shadow(1.0, 2.0, p.shadow)
        } else {
            seg.hover(p.hover)
        };
        r = r.child(seg);
    }
    r
}

/// An on/off switch. The caller adds the press.
pub fn switch(on: bool, key: &str, p: &Pal) -> El {
    let knob = el()
        .size(14.0, 14.0)
        .radius(7.0)
        .fill(if on { p.paper } else { p.ink3 });
    let mut track = row()
        .key(key.to_string())
        .size(32.0, 18.0)
        .px(2.0)
        .center_y()
        .radius(9.0)
        .cursor(Cursor::Pointer)
        .fill(if on { p.ink } else { Color::TRANSPARENT })
        .border(1.0, if on { p.ink } else { p.ink4 });
    if on {
        track = track.child(el().grow(1.0));
    }
    track.child(knob)
}

/// A setting: its name and a line saying what it does on the left, its control on the right.
pub fn setting_row(label: &str, about: &str, control: El, p: &Pal) -> El {
    let mut words = col().grow(1.0).min_w(0.0).gap(2.0).child(
        text(label.to_string())
            .font(crate::theme::UI_MED)
            .color(p.ink),
    );
    if !about.is_empty() {
        words = words.child(text(about.to_string()).font(SMALL).color(p.ink3).wrap());
    }
    row()
        .w_full()
        .py(12.0)
        .gap(24.0)
        .center_y()
        .border_sides([0.0, 0.0, 1.0, 0.0], p.rule)
        .child(words)
        .child(control)
}

/// "just now", "5 minutes ago", "3 hours ago", "yesterday", "12 days ago": when something
/// happened, from unix seconds.
pub fn ago(at: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let d = (now - at).max(0);
    match d {
        0..60 => "just now".into(),
        60..3600 => format!("{} ago", plural((d / 60) as usize, "minute")),
        3600..86400 => format!("{} ago", plural((d / 3600) as usize, "hour")),
        86400..172800 => "yesterday".into(),
        _ => format!("{} ago", plural((d / 86400) as usize, "day")),
    }
}

/// 4619 → "4,619".
pub fn thousands(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}
