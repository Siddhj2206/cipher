//! Fonts, type scale and the two palettes, copied from the prototype (`docs/design/desktop-ux.md`
//! › Look). Colour is chosen by role, never by hex, in the views.

use rune::color::hex;
use rune::tok::{Family, TextStyle};
use rune::{Color, Icon};

use crate::prefs::{Reading, Typeface};

pub const SERIF: Family =
    Family::Named("Noto Serif, Noto Serif CJK KR, Noto Serif KR, AppleMyungjo, Batang, serif");
pub const CJK: Family =
    Family::Named("Noto Serif CJK KR, Noto Serif KR, AppleMyungjo, Batang, Noto Serif, serif");
pub const INTERLINEAR: TextStyle = TextStyle::new(14.0, 400.0).lh(23.0).family(CJK);
pub const SANS: Family = Family::Named(
    "Noto Sans, Segoe UI, Helvetica Neue, Noto Sans CJK KR, Apple SD Gothic Neo, Malgun Gothic, sans-serif",
);

/// The reading column's text, from the Reading preferences.
pub fn body(r: &Reading) -> TextStyle {
    TextStyle::new(r.size, 400.0)
        .lh((r.size * r.leading).round())
        .family(family(r))
}

fn family(r: &Reading) -> Family {
    match r.typeface {
        Typeface::Serif => SERIF,
        Typeface::Sans => SANS,
    }
}

/// The source text: CJK runs a point smaller than the Translation looks right beside it.
pub fn src_body(r: &Reading) -> TextStyle {
    let size = r.size - 1.0;
    TextStyle::new(size, 400.0)
        .lh((r.size * r.leading).round())
        .family(CJK)
}

pub fn heading(r: &Reading) -> TextStyle {
    let size = (r.size * 1.33).round();
    TextStyle::new(size, 600.0)
        .lh((size * 1.4).round())
        .family(family(r))
}

pub const UI: TextStyle = TextStyle::new(13.0, 400.0);
pub const UI_MED: TextStyle = TextStyle::new(13.0, 550.0);
pub const SMALL: TextStyle = TextStyle::new(12.0, 400.0);
pub const LABEL: TextStyle = TextStyle::new(10.5, 650.0).tracking(1.4);
pub const SPARK: TextStyle = TextStyle::new(7.5, 400.0)
    .lh(14.0)
    .family(Family::Named("DejaVu Sans Mono"));

pub const LEFT_W: f32 = 272.0;
pub const RIGHT_W: f32 = 320.0;
pub const HEADER_H: f32 = 52.0;
pub const SCRUBBER_W: f32 = 44.0;
pub const GUTTER_W: f32 = 56.0;

fn c(h: &str) -> Color {
    hex(h).expect("palette colours are valid hex")
}

/// Two themes from one set of roles: warm paper by day, warm ink-dark by night.
pub struct Pal {
    pub paper: Color,
    pub side: Color,
    pub card: Color,
    pub ink: Color,
    pub ink2: Color,
    pub ink3: Color,
    pub ink4: Color,
    pub rule: Color,
    pub hover: Color,
    pub selected: Color,
    pub red: Color,
    pub red_soft: Color,
    pub red_tint: Color,
    pub on_red: Color,
    pub green: Color,
    pub green_tint: Color,
    pub amber: Color,
    pub highlight: Color,
    pub term_bg: Color,
    /// The paragraph under review.
    pub focus_bg: Color,
    pub primary_hover: Color,
    /// The dark bars: the selection chip and the review bar.
    pub bar: Color,
    pub bar_fg: Color,
    pub bar_fg2: Color,
    pub bar_hover: Color,
    pub scrim: Color,
    pub shadow: Color,
}

pub fn pal(dark: bool) -> Pal {
    if dark {
        Pal {
            paper: c("#1A1916"),
            side: c("#151412"),
            card: c("#22201C"),
            ink: c("#E9E4D9"),
            ink2: c("#B5AE9F"),
            ink3: c("#7F796D"),
            ink4: c("#4B4740"),
            rule: c("#2D2A25"),
            hover: c("#26241F"),
            selected: c("#2E2B26"),
            red: c("#E5694C"),
            red_soft: c("#A65141"),
            red_tint: c("#E5694C2B"),
            on_red: c("#1A1916"),
            green: c("#86C496"),
            green_tint: c("#86C4962B"),
            amber: c("#D9A13A"),
            highlight: c("#D9A93E5C"),
            term_bg: c("#312D27"),
            focus_bg: c("#23211D"),
            primary_hover: c("#D6D0C4"),
            bar: c("#2C2A25"),
            bar_fg: c("#E9E4D9"),
            bar_fg2: c("#8F887B"),
            bar_hover: c("#3A3731"),
            scrim: c("#00000080"),
            shadow: c("#00000099"),
        }
    } else {
        Pal {
            paper: c("#F3EFE6"),
            side: c("#EDE8DD"),
            card: c("#FBF9F4"),
            ink: c("#1E1C18"),
            ink2: c("#5B564C"),
            ink3: c("#948D7F"),
            ink4: c("#C9C2B3"),
            rule: c("#E0DACD"),
            hover: c("#E6E0D3"),
            selected: c("#E1DACB"),
            red: c("#B4361E"),
            red_soft: c("#D9806D"),
            red_tint: c("#B4361E24"),
            on_red: c("#FFF6F2"),
            green: c("#2E6A3A"),
            green_tint: c("#2E6A3A2E"),
            amber: c("#C98A1B"),
            highlight: c("#F4C84A8C"),
            term_bg: c("#E6DECC"),
            focus_bg: c("#ECE5D5"),
            primary_hover: c("#3A3730"),
            bar: c("#1E1C18"),
            bar_fg: c("#F3EFE6"),
            bar_fg2: c("#A39C8E"),
            bar_hover: c("#3A3730"),
            scrim: c("#1E1C1838"),
            shadow: c("#1E1C1830"),
        }
    }
}

macro_rules! lucide {
    ($name:ident, $id:literal, $body:literal) => {
        pub static $name: Icon = Icon {
            name: $id,
            view: 24.0,
            body: concat!(
                r#"<g fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</g>"
            ),
        };
    };
}

lucide!(
    I_LANG,
    "l-languages",
    r#"<path d="m5 8 6 6"/><path d="m4 14 6-6 2-3"/><path d="M2 5h12"/><path d="M7 2h1"/><path d="m22 22-5-10-5 10"/><path d="M14 18h6"/>"#
);
lucide!(
    I_X,
    "l-x",
    r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#
);
lucide!(
    I_ARROW,
    "l-arrow-right",
    r#"<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>"#
);
lucide!(
    I_SUN,
    "l-sun",
    r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>"#
);
lucide!(
    I_MOON,
    "l-moon",
    r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>"#
);
lucide!(
    I_LEFT_PANEL,
    "l-panel-left",
    r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>"#
);
lucide!(
    I_RIGHT_PANEL,
    "l-panel-right",
    r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/>"#
);
lucide!(
    I_USERS,
    "l-users",
    r#"<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>"#
);
lucide!(
    I_BOOK,
    "l-book-open",
    r#"<path d="M12 7v14"/><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z"/>"#
);
lucide!(
    I_PEN,
    "l-pen-line",
    r#"<path d="M12 20h9"/><path d="M16.376 3.622a1 1 0 0 1 3.002 3.002L7.368 18.635a2 2 0 0 1-.855.506l-2.872.838a.5.5 0 0 1-.62-.62l.838-2.872a2 2 0 0 1 .506-.854z"/>"#
);
lucide!(I_UP, "l-chevron-up", r#"<path d="m18 15-6-6-6 6"/>"#);
lucide!(
    I_HISTORY,
    "l-history",
    r#"<path d="M3 12a9 9 0 1 0 2.6-6.4L3 8"/><path d="M3 3v5h5M12 7v5l3 2"/>"#
);
lucide!(I_DOWN, "l-chevron-down", r#"<path d="m6 9 6 6 6-6"/>"#);
lucide!(
    I_PLUS,
    "l-plus",
    r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#
);
lucide!(
    I_FOLDER,
    "l-folder-open",
    r#"<path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"/>"#
);
lucide!(
    I_FILE,
    "l-file",
    r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>"#
);
lucide!(
    I_PLUG,
    "l-plug",
    r#"<path d="M12 22v-5"/><path d="M9 8V2"/><path d="M15 8V2"/><path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z"/>"#
);
lucide!(
    I_SHELF,
    "l-library",
    r#"<path d="m16 6 4 14"/><path d="M12 6v14"/><path d="M8 8v12"/><path d="M4 4v16"/>"#
);
lucide!(
    I_GEAR,
    "l-settings",
    r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>"#
);
lucide!(
    I_TYPE,
    "l-type",
    r#"<path d="M4 7V4h16v3"/><path d="M9 20h6"/><path d="M12 4v16"/>"#
);
