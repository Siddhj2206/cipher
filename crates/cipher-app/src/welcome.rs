//! The first run (`docs/design/desktop-ux.md` › Welcome): a title page that connects a model
//! and starts the first Book, in place. Shown once, while the Shelf is empty; Settings › About
//! shows it again.

use crate::settings::{Kind, Settings};
use crate::state::State;

pub struct Welcome {
    /// A way to connect was chosen, so its form shows.
    pub picked: bool,
}

impl State {
    pub fn show_welcome(&mut self) {
        if self.home.is_none() {
            self.go_shelf();
        }
        self.close_settings();
        self.welcome = Some(Welcome { picked: false });
        self.settings = Some(Settings::load());
        self.start_new_book();
    }

    /// API key or ChatGPT: the Endpoint form opens below the choice.
    pub fn welcome_pick(&mut self, kind: Kind) {
        if let Some(form) = &mut self.settings {
            form.kind = kind;
        }
        if let Some(w) = &mut self.welcome {
            w.picked = true;
        }
    }

    /// The saved default Endpoint and its model, once one is connected.
    pub fn connected(&self) -> Option<String> {
        let f = self.settings.as_ref()?;
        let id = f.default.as_deref()?;
        let e = f.all.iter().find(|e| e.id == id)?;
        Some(format!("{} · {}", e.name, e.translate_model))
    }

    /// Done with the welcome: it won't show again.
    pub fn finish_welcome(&mut self) {
        if self.welcome.take().is_some() {
            self.settings = None;
            self.prefs.onboarded = true;
        }
    }
}
