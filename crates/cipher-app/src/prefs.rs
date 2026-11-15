//! The app's own preferences (`docs/design/desktop-ux.md` › Settings): appearance, reading
//! type, the window and the sidebars. They live in `prefs.json` beside `endpoints.json`, apply
//! the moment they change and are written at once (a temporary file, then a rename). Every
//! field has a default, so a file from an older or newer build still reads; one that can't be
//! read at all is left alone and the defaults are used. Headless runs neither read nor write it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Light, dark, or whatever the desktop says.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            Appearance::System => system_dark,
            Appearance::Light => false,
            Appearance::Dark => true,
        }
    }
}

/// The Shelf's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShelfSort {
    /// Most recently read first.
    #[default]
    Recent,
    Title,
}

/// The reading column's typeface.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Typeface {
    #[default]
    Serif,
    Sans,
}

/// How the reading page looks. Sizes are points.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Reading {
    pub typeface: Typeface,
    pub size: f32,
    /// Line height as a multiple of the size.
    pub leading: f32,
    /// The column's width.
    pub width: f32,
}

impl Default for Reading {
    fn default() -> Reading {
        Reading {
            typeface: Typeface::Serif,
            size: 18.0,
            leading: 1.72,
            width: 640.0,
        }
    }
}

impl Reading {
    pub const SIZES: (f32, f32) = (14.0, 26.0);
    pub const WIDTHS: [(f32, &'static str); 3] =
        [(560.0, "Narrow"), (640.0, "Medium"), (760.0, "Wide")];
    pub const LEADINGS: [(f32, &'static str); 3] =
        [(1.5, "Tight"), (1.72, "Normal"), (1.95, "Loose")];

    /// One step bigger (`up`) or smaller, within the limits.
    pub fn step_size(&mut self, up: bool) {
        let next = self.size + if up { 1.0 } else { -1.0 };
        self.size = next.clamp(Self::SIZES.0, Self::SIZES.1);
    }
}

/// The window as it was when it last closed.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl Default for WindowState {
    fn default() -> WindowState {
        WindowState {
            width: 1480.0,
            height: 920.0,
            maximized: false,
        }
    }
}

impl WindowState {
    pub const MIN: (f32, f32) = (760.0, 520.0);

    /// The size to open at: never below the minimum, never absurdly large.
    pub fn size(&self) -> (f32, f32) {
        (
            self.width.clamp(Self::MIN.0, 7680.0),
            self.height.clamp(Self::MIN.1, 4320.0),
        )
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub appearance: Appearance,
    pub reading: Reading,
    /// The whole interface's scale, on top of the display's.
    pub zoom: f32,
    pub window: WindowState,
    pub contents_open: bool,
    pub context_open: bool,
    /// Open the Book that was open when the app last closed, instead of the Shelf.
    pub reopen_last: bool,
    pub last_book: Option<PathBuf>,
    pub shelf_sort: ShelfSort,
    /// The welcome has been seen through (or skipped).
    pub onboarded: bool,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            appearance: Appearance::System,
            reading: Reading::default(),
            zoom: 1.0,
            window: WindowState::default(),
            contents_open: true,
            context_open: true,
            reopen_last: true,
            last_book: None,
            shelf_sort: ShelfSort::Recent,
            onboarded: false,
        }
    }
}

impl Prefs {
    pub const ZOOMS: [f32; 7] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.4, 1.6];

    /// `prefs.json` beside `endpoints.json`.
    pub fn default_path() -> Option<PathBuf> {
        Some(
            cipher_llm::Endpoints::default_path()?
                .parent()?
                .join("prefs.json"),
        )
    }

    /// The saved preferences, or the defaults when there are none or they can't be read.
    pub fn load(path: &Path) -> Prefs {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)
            .and_then(|()| std::fs::rename(&tmp, path))
            .map_err(|e| format!("Could not save {}: {e}", path.display()))
    }

    /// One zoom step in (`up`) or out from the current one.
    pub fn step_zoom(&mut self, up: bool) {
        let i = Self::ZOOMS
            .iter()
            .position(|z| (z - self.zoom).abs() < 0.01)
            .unwrap_or(2);
        let j = if up {
            (i + 1).min(Self::ZOOMS.len() - 1)
        } else {
            i.saturating_sub(1)
        };
        self.zoom = Self::ZOOMS[j];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_or_old_file_reads_with_defaults_for_the_rest() {
        let p: Prefs =
            serde_json::from_str(r#"{"appearance":"dark","reading":{"size":21},"unknown":1}"#)
                .unwrap();
        assert_eq!(p.appearance, Appearance::Dark);
        assert_eq!(p.reading.size, 21.0);
        assert_eq!(p.reading.width, Reading::default().width);
        assert!(p.contents_open);
    }

    #[test]
    fn saves_and_loads_and_a_broken_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/prefs.json");
        let p = Prefs {
            appearance: Appearance::Light,
            window: WindowState {
                maximized: true,
                ..WindowState::default()
            },
            ..Prefs::default()
        };
        p.save(&path).unwrap();
        assert_eq!(Prefs::load(&path), p);
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Prefs::load(&path), Prefs::default());
    }

    #[test]
    fn steps_stay_in_range() {
        let mut p = Prefs::default();
        for _ in 0..20 {
            p.step_zoom(true);
            p.reading.step_size(true);
        }
        assert_eq!(p.zoom, 1.6);
        assert_eq!(p.reading.size, Reading::SIZES.1);
        assert!(!Appearance::System.dark(false) && Appearance::System.dark(true));
        assert!(
            WindowState {
                width: 10.0,
                height: 10.0,
                maximized: false
            }
            .size()
                == WindowState::MIN
        );
    }
}
