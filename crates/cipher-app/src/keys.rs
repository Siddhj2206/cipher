//! Every keyboard shortcut, once: the shortcuts sheet (Ctrl+/), Settings › Shortcuts and the
//! tooltips read this table. `state::on_key` is what makes them work.

/// (group, keys, what it does). Keys are written as the sheet shows them, `+` between keys.
pub const SHORTCUTS: &[(&str, &str, &str)] = &[
    ("General", "Ctrl+K", "Search commands and Chapters"),
    ("General", "Ctrl+,", "Settings"),
    ("General", "Ctrl+/", "Keyboard shortcuts"),
    ("General", "Ctrl+O", "Shelf"),
    ("General", "Ctrl+N", "New chapter (on the Shelf: new book)"),
    ("General", "Ctrl+D", "Light or dark"),
    ("General", "F11", "Full screen"),
    ("General", "Ctrl+Q", "Quit"),
    ("Reading", "Space", "Down a screen"),
    ("Reading", "Ctrl+F", "Find in book"),
    ("Reading", "Enter / Shift+Enter", "Next / previous match"),
    ("Reading", "Esc", "Close find"),
    ("Reading", "Shift+Space", "Up a screen"),
    ("Reading", "Alt+→", "Next Chapter"),
    ("Reading", "Alt+←", "Previous Chapter"),
    ("Reading", "Ctrl+T", "Source or Translation"),
    ("Reading", "Ctrl+I", "Source under each paragraph"),
    ("Reading", "Ctrl+= / Ctrl+-", "Bigger or smaller text"),
    ("Reading", "Ctrl+0", "Text size back to normal"),
    ("Panels", "Ctrl+\\", "Contents"),
    ("Panels", "Ctrl+J", "Terms and Suggestions"),
    (
        "Panels",
        "← / → on divider",
        "Resize a sidebar (Tab to divider)",
    ),
    ("Panels", "Ctrl+G", "Cast"),
    ("Panels", "Ctrl+H", "History"),
    ("History", "↑ / ↓", "Select a Revision"),
    ("History", "Tab / Enter", "Focus or activate an action"),
    ("History", "Esc", "Back to reading"),
    ("Adding", "Ctrl+V", "Paste a chapter onto a new page"),
    ("Adding", "Ctrl+Enter", "Add the chapter on the page"),
    ("Adding", "Esc", "Back to reading; the page keeps its draft"),
    ("Fixing", "Type over a selection", "Suggest a change to it"),
    ("Fixing", "J / K", "Next or previous Proposed Edit"),
    ("Fixing", "A / X", "Accept or reject it"),
    ("Fixing", "Shift+A", "Accept the rest"),
    ("Fixing", "Esc", "Leave Review"),
];

/// The groups in the order they first appear.
pub fn groups() -> Vec<&'static str> {
    let mut out: Vec<&str> = Vec::new();
    for (g, _, _) in SHORTCUTS {
        if !out.contains(g) {
            out.push(g);
        }
    }
    out
}
