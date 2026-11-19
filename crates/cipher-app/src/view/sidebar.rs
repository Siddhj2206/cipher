use rune::{Color, Cursor, Drag, El, Key, Phase, Tx, el};

use crate::sidebars::Sidebar;
use crate::state::State;
use crate::theme::Pal;

const GRIP: f32 = 10.0;
const STEP: f32 = 16.0;

pub fn grip(s: &State, p: &Pal, side: Sidebar, width: Option<f32>) -> Option<El> {
    let (key, label, sign, order) = match side {
        Sidebar::Contents => ("resize-contents", "Resize Contents sidebar", 1.0, 1),
        Sidebar::Context => ("resize-context", "Resize Context sidebar", -1.0, 2),
    };
    let grip = el()
        .key(key)
        .on_drag(move |s: &mut State, d: &mut Drag, tx| match d.phase {
            Phase::Down => d.data = s.sidebar_layout().width(side).unwrap_or(0.0),
            Phase::Move => s.resize_sidebar(side, d.data + sign * d.dx, false, tx),
            Phase::Up => {
                if !d.cancelled() && d.dx != 0.0 {
                    s.resize_sidebar(side, d.data + sign * d.dx, false, tx);
                }
                s.finish_sidebar_drag(side, d.data, d.cancelled(), tx);
            }
        });
    let Some(width) = width else {
        // If the window narrows mid-drag, keep the captured handler until it can clean up.
        return s
            .sidebar_dragging(side)
            .then(|| grip.size(0.0, 0.0).a11y_hidden());
    };
    let at = Some(width - GRIP / 2.0);
    let (left, right) = if side == Sidebar::Contents {
        (at, None)
    } else {
        (None, at)
    };
    Some(
        grip.pin(left, Some(0.0), right, Some(0.0))
            .w(GRIP)
            .fill(Color::TRANSPARENT)
            .hover(p.rule)
            .press(p.red_tint)
            .focus_ring(1.0, -1.0, p.red)
            .tab_index(order)
            .cursor(Cursor::ResizeColumn)
            .keeps_text_selection()
            .role("separator")
            .label_static(label)
            .range_value(
                f64::from(width),
                f64::from(side.min_width()),
                f64::from(s.sidebar_max_width(side)),
            )
            .range_step(f64::from(STEP))
            .help(format!("{label} · Drag, or Tab here then use ← / →"))
            .on_key(move |s: &mut State, k, tx| {
                if k.mods.cmd || k.mods.ctrl || k.mods.alt || k.mods.logo || k.mods.shift {
                    return false;
                }
                let delta = match k.key {
                    Key::Left => -sign * STEP,
                    Key::Right => sign * STEP,
                    _ => return false,
                };
                step(s, side, delta, tx);
                true
            })
            .on_step(move |s: &mut State, n, tx| step(s, side, n as f32 * STEP, tx))
            .on_step_limit(move |s: &mut State, max, tx| {
                let width = if max {
                    s.sidebar_max_width(side)
                } else {
                    side.min_width()
                };
                s.resize_sidebar(side, width, true, tx);
            }),
    )
}

fn step(s: &mut State, side: Sidebar, delta: f32, tx: &mut Tx) {
    if let Some(width) = s.sidebar_layout().width(side) {
        s.resize_sidebar(side, width + delta, true, tx);
    }
}
