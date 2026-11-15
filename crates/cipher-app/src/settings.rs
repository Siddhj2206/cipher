//! Endpoint settings (`docs/design/desktop-ux.md` › Endpoint settings): one Endpoint, set up
//! with an API key or by signing in with ChatGPT, a test call, and Save, which restarts the
//! worker on it.
//!
//! Keys and tokens go to the OS keychain, the rest to `endpoints.json` (ADR 0006). Headless runs
//! never read or write either.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cipher_llm::{
    ChatGptSession, Effort, Endpoint, Endpoints, InMemory, Issuer, Keychain, Protocol, Request,
    Secrets, SignIn,
};
use rune::{TextEdit, Tx};

use crate::fix::work::Msg;
use crate::state::{Section, State};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    ApiKey,
    ChatGpt,
}

/// What a test call or a sign-in is doing.
#[derive(Clone, PartialEq, Debug)]
pub enum Status {
    Idle,
    Busy(String),
    Ok(String),
    Failed(String),
}

pub const EFFORTS: [(Effort, &str); 5] = [
    (Effort::ProviderDefault, "Default"),
    (Effort::Minimal, "Minimal"),
    (Effort::Low, "Low"),
    (Effort::Medium, "Medium"),
    (Effort::High, "High"),
];

pub struct Settings {
    pub kind: Kind,
    /// For an API key: Chat Completions or Responses.
    pub protocol: Protocol,
    pub(crate) id: String,
    pub name: TextEdit,
    pub base_url: TextEdit,
    pub key: TextEdit,
    pub translate: TextEdit,
    /// More models Books may pick on this Endpoint, comma-separated.
    pub others: TextEdit,
    pub small: TextEdit,
    pub effort: Effort,
    /// The keychain already holds a key for this Endpoint, so an empty field keeps it.
    pub has_key: bool,
    /// Signed in with ChatGPT as this account.
    pub signed_in: Option<String>,
    pub status: Status,
    /// Every Endpoint as saved, for the list above the form.
    pub all: Vec<Endpoint>,
    /// The saved default Endpoint's id.
    pub default: Option<String>,
    /// The form is for an Endpoint not saved yet.
    pub is_new: bool,
    /// Models the signed-in plan offers, fetched when the form opens; empty until then.
    pub available: Vec<String>,
}

fn field(text: &str) -> TextEdit {
    TextEdit::new(text)
}

fn non_empty(t: &TextEdit) -> Option<String> {
    let t = t.text.trim();
    (!t.is_empty()).then(|| t.to_string())
}

impl Settings {
    /// The form for the default Endpoint, or a blank one when there is none.
    pub fn load() -> Settings {
        Settings::load_for(None)
    }

    /// The form for the Endpoint `id` (the default when `None`), with the list of them all.
    pub fn load_for(id: Option<&str>) -> Settings {
        let saved = crate::live()
            .then(|| Endpoints::default_path().and_then(|p| Endpoints::load(&p).ok()))
            .flatten()
            .unwrap_or_default();
        let current = match id {
            Some(id) => saved.endpoints.iter().find(|e| e.id == id).cloned(),
            None => saved.current().cloned(),
        };
        let mut s = Settings::blank();
        s.default = saved.current().map(|e| e.id.clone());
        s.all = saved.endpoints;
        let Some(e) = current else { return s };
        s.fill(e);
        s
    }

    /// A form for a new Endpoint.
    pub fn blank() -> Settings {
        Settings {
            kind: Kind::ApiKey,
            protocol: Protocol::ChatCompletions,
            id: format!("endpoint-{}", unix_now()),
            name: field(""),
            base_url: field(""),
            key: TextEdit::new("").secure(),
            translate: field(""),
            small: field(""),
            others: field(""),
            effort: Effort::ProviderDefault,
            has_key: false,
            signed_in: None,
            status: Status::Idle,
            all: Vec::new(),
            default: None,
            is_new: true,
            available: Vec::new(),
        }
    }

    fn fill(&mut self, e: Endpoint) {
        let s = self;
        s.is_new = false;
        s.kind = if e.protocol == Protocol::ChatGpt {
            Kind::ChatGpt
        } else {
            Kind::ApiKey
        };
        if e.protocol != Protocol::ChatGpt {
            s.protocol = e.protocol;
        }
        s.name = field(&e.name);
        s.base_url = field(&e.base_url);
        s.translate = field(&e.translate_model);
        s.others = field(&e.models.join(", "));
        s.small = field(e.small_model.as_deref().unwrap_or(""));
        s.effort = e.reasoning;
        s.has_key = Keychain.get(&e.api_key_id()).ok().flatten().is_some();
        s.signed_in = ChatGptSession::stored(&Keychain, &e.id)
            .ok()
            .flatten()
            .map(|c| c.describe());
        s.id = e.id;
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The Endpoint the form describes, or what is missing.
    pub fn endpoint(&self) -> Result<Endpoint, String> {
        let translate_model = non_empty(&self.translate).ok_or("Name the default model.")?;
        let (protocol, base_url) = match self.kind {
            Kind::ChatGpt => (Protocol::ChatGpt, String::new()),
            Kind::ApiKey => (
                self.protocol,
                non_empty(&self.base_url).ok_or("Give the base URL.")?,
            ),
        };
        let name = non_empty(&self.name).unwrap_or_else(|| match self.kind {
            Kind::ChatGpt => "ChatGPT".into(),
            Kind::ApiKey => host(&base_url),
        });
        Ok(Endpoint {
            id: self.id.clone(),
            name,
            protocol,
            base_url,
            translate_model: translate_model.clone(),
            small_model: non_empty(&self.small),
            reasoning: self.effort,
            models: self
                .others
                .text
                .split(',')
                .map(str::trim)
                .filter(|m| !m.is_empty() && *m != translate_model.as_str())
                .map(String::from)
                .collect(),
        })
    }

    /// Ready to send: the fields are filled, and a ChatGPT Endpoint has signed in.
    pub fn ready(&self) -> bool {
        self.endpoint().is_ok() && (self.kind == Kind::ApiKey || self.signed_in.is_some())
    }

    pub fn busy(&self) -> bool {
        matches!(self.status, Status::Busy(_))
    }
}

/// `https://api.example.com/v1` → `api.example.com`.
fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split('/').next().unwrap_or(rest).to_string()
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl State {
    /// Settings at Translation, where the Endpoint is set up.
    pub fn open_settings(&mut self) {
        self.open_settings_at(Section::Translation);
    }

    /// Opens Settings at `section`. The Endpoint form loads the first time Translation shows,
    /// since reading it asks the keychain.
    pub fn open_settings_at(&mut self, section: Section) {
        self.palette = None;
        self.shortcuts_open = false;
        self.settings_page = Some(section);
        if section == Section::Translation && self.settings.is_none() {
            self.settings = Some(Settings::load());
            self.list_models();
        }
    }

    pub fn close_settings(&mut self) {
        self.settings_page = None;
        self.settings = None;
    }

    /// Back to the Endpoint as saved, dropping what was typed.
    pub fn reload_endpoint(&mut self) {
        let id = self
            .settings
            .as_ref()
            .filter(|f| !f.is_new)
            .map(|f| f.id.clone());
        self.settings = Some(Settings::load_for(id.as_deref()));
    }

    /// Edits the saved Endpoint `id` in the form.
    pub fn edit_endpoint(&mut self, id: &str) {
        self.settings = Some(Settings::load_for(Some(id)));
    }

    /// A blank form for another Endpoint, keeping the list.
    pub fn add_endpoint(&mut self) {
        let (all, default) = match &self.settings {
            Some(f) => (f.all.clone(), f.default.clone()),
            None => (Vec::new(), None),
        };
        let mut form = Settings::blank();
        form.all = all;
        form.default = default;
        self.settings = Some(form);
    }

    /// Makes `id` the Endpoint Books use unless they chose another.
    pub fn make_default_endpoint(&mut self, id: &str) {
        let editing = self.settings.as_ref().map(|f| f.id.clone());
        if !rune::is_headless() {
            let Some(path) = Endpoints::default_path() else {
                return;
            };
            let mut all = Endpoints::load(&path).unwrap_or_default();
            all.default = Some(id.to_string());
            if let Err(e) = all.save(&path) {
                return self.settings_failed(format!("Could not save {}: {e}", path.display()));
            }
            self.restart_worker();
        }
        match &mut self.settings {
            Some(f) if rune::is_headless() => f.default = Some(id.to_string()),
            _ => self.settings = Some(Settings::load_for(editing.as_deref())),
        }
    }

    /// Forgets the Endpoint `id` and its key. Books that chose it go back to the default.
    pub fn remove_endpoint(&mut self, id: &str) {
        if !rune::is_headless() {
            let Some(path) = Endpoints::default_path() else {
                return;
            };
            let mut all = Endpoints::load(&path).unwrap_or_default();
            if let Some(e) = all.endpoints.iter().find(|e| e.id == id) {
                let _ = Keychain.delete(&e.api_key_id());
            }
            all.endpoints.retain(|e| e.id != id);
            if all.default.as_deref() == Some(id) {
                all.default = all.endpoints.first().map(|e| e.id.clone());
            }
            if let Err(e) = all.save(&path) {
                return self.settings_failed(format!("Could not save {}: {e}", path.display()));
            }
            self.restart_worker();
            self.settings = Some(Settings::load());
        } else if let Some(f) = &mut self.settings {
            f.all.retain(|e| e.id != id);
        }
    }

    /// Opens or closes the header's model menu, reading the Endpoints afresh.
    pub fn toggle_model_menu(&mut self) {
        if self.model_menu.take().is_some() {
            return;
        }
        let saved = crate::live()
            .then(|| Endpoints::default_path().and_then(|p| Endpoints::load(&p).ok()))
            .flatten()
            .unwrap_or_default();
        let default = saved.current().map(|e| e.id.clone());
        self.model_menu = Some((saved.endpoints, default));
    }

    /// This Book translates with `choice` from now on (`None`: the app's default).
    pub fn choose_model(&mut self, choice: Option<cipher_book::ModelChoice>) {
        self.model_menu = None;
        if let Err(e) = self.lib.set_model_choice(choice.as_ref()) {
            self.toast = Some(format!("Could not save the model: {e}"));
            return;
        }
        self.restart_worker();
        self.toast = Some(match choice {
            Some(c) => format!("This book now translates with {}", c.model),
            None => "This book now uses the default model".into(),
        });
        self.toast_undo = None;
    }

    /// Restarts the open Book's worker, which picks its Endpoint and model again.
    pub fn restart_worker(&mut self) {
        if self.lib.is_saved() && crate::live() {
            let book = self.lib.path().to_path_buf();
            self.bg.restart(&book);
        }
    }

    fn settings_failed(&mut self, why: String) {
        if let Some(form) = &mut self.settings {
            form.status = Status::Failed(why);
        }
    }

    /// Writes the key and the Endpoint, makes it current and restarts the worker on it.
    pub fn save_settings(&mut self) {
        let Some(form) = &self.settings else { return };
        let endpoint = match form.endpoint() {
            Ok(e) => e,
            Err(why) => return self.settings_failed(why),
        };
        if !form.ready() {
            return self.settings_failed("Sign in with ChatGPT first.".into());
        }
        if rune::is_headless() {
            return self.endpoint_saved(&endpoint.name);
        }
        if form.kind == Kind::ApiKey
            && let Some(key) = non_empty(&form.key)
            && let Err(e) = Keychain.set(&endpoint.api_key_id(), &key)
        {
            return self.settings_failed(format!("Could not store the key in the keychain: {e}"));
        }
        let Some(path) = Endpoints::default_path() else {
            return self.settings_failed("No config folder: HOME is not set.".into());
        };
        let mut all = Endpoints::load(&path).unwrap_or_default();
        match all.endpoints.iter_mut().find(|e| e.id == endpoint.id) {
            Some(e) => *e = endpoint.clone(),
            None => all.endpoints.push(endpoint.clone()),
        }
        // The first Endpoint becomes the default; later ones are made default explicitly.
        if all.current().is_none_or(|e| e.id == endpoint.id) || all.default.is_none() {
            all.default = Some(endpoint.id.clone());
        }
        if let Err(e) = all.save(&path) {
            return self.settings_failed(format!("Could not save {}: {e}", path.display()));
        }
        self.restart_worker();
        self.endpoint_saved(&endpoint.name);
    }

    /// The form again as saved, saying so.
    fn endpoint_saved(&mut self, name: &str) {
        let mut form = if rune::is_headless() {
            let mut f = self.settings.take().expect("the form was open");
            if let Ok(e) = f.endpoint() {
                f.all.retain(|x| x.id != e.id);
                f.all.push(e);
            }
            f.default.get_or_insert_with(|| f.id.clone());
            f.is_new = false;
            f
        } else {
            let id = self.settings.as_ref().map(|f| f.id.clone());
            Settings::load_for(id.as_deref())
        };
        form.key = TextEdit::new("").secure();
        form.status = Status::Ok(format!("Saved {name}."));
        self.settings = Some(form);
    }

    /// One small call to the translate model, with the key as typed (not yet stored).
    pub fn test_settings(&mut self) {
        let Some(form) = self.settings.as_mut().filter(|f| !f.busy()) else {
            return;
        };
        let endpoint = match form.endpoint() {
            Ok(e) => e,
            Err(why) => {
                form.status = Status::Failed(why);
                return;
            }
        };
        form.status = Status::Busy("Testing…".into());
        if rune::is_headless() {
            return;
        }
        let secrets: Arc<dyn Secrets> = match non_empty(&form.key) {
            Some(key) if form.kind == Kind::ApiKey => {
                let mem = InMemory::new();
                let _ = mem.set(&endpoint.api_key_id(), &key);
                Arc::new(mem)
            }
            _ => Arc::new(Keychain),
        };
        self.bg
            .spawn(move || Msg::Tested(test_call(&endpoint, secrets)));
    }

    /// Signs in with ChatGPT in the browser. The URL comes back to open; the Credentials go to
    /// the keychain under this Endpoint.
    pub fn sign_in(&mut self) {
        let Some(form) = self.settings.as_mut().filter(|f| !f.busy()) else {
            return;
        };
        form.status = Status::Busy("Waiting for the browser…".into());
        if rune::is_headless() {
            return;
        }
        let id = form.id.clone();
        let outbox = self.bg.outbox();
        self.bg.spawn(move || {
            let result = (|| -> Result<(String, Vec<String>), String> {
                let previous = ChatGptSession::stored(&Keychain, &id).ok().flatten();
                let email = previous.as_ref().and_then(|p| p.email.as_deref());
                let sign_in = SignIn::start(Issuer::openai(), email).map_err(|e| e.to_string())?;
                outbox.send(Msg::OpenUrl(sign_in.url().to_string()));
                let creds = sign_in
                    .finish(Duration::from_secs(300))
                    .map_err(|e| e.to_string())?;
                ChatGptSession::save(&Keychain, &id, &creds).map_err(|e| e.to_string())?;
                // The plan's models fill the form; without them the user types one.
                let models = ChatGptSession::open(Issuer::openai(), Arc::new(Keychain), &id)
                    .and_then(|s| s.models())
                    .map(|m| m.into_iter().map(|(slug, _)| slug).collect())
                    .unwrap_or_default();
                Ok((creds.describe(), models))
            })();
            Msg::SignedIn(result)
        });
    }

    pub fn on_tested(&mut self, result: Result<String, String>) {
        if let Some(form) = &mut self.settings {
            form.status = match result {
                Ok(m) => Status::Ok(m),
                Err(m) => Status::Failed(m),
            };
        }
    }

    /// Asks the signed-in plan which models it offers, for the form's choices.
    pub fn list_models(&mut self) {
        let Some(form) = &self.settings else { return };
        if form.kind != Kind::ChatGpt || form.signed_in.is_none() || !crate::live() {
            return;
        }
        let id = form.id.clone();
        self.bg.spawn(move || {
            let models = ChatGptSession::open(Issuer::openai(), Arc::new(Keychain), &id)
                .and_then(|s| s.models())
                .map(|m| m.into_iter().map(|(slug, _)| slug).collect())
                .map_err(|e| e.to_string());
            Msg::ModelsListed(id, models)
        });
    }

    pub fn on_models_listed(&mut self, id: String, result: Result<Vec<String>, String>) {
        if let Some(form) = self.settings.as_mut().filter(|f| f.id == id)
            && let Ok(models) = result
        {
            form.available = models;
        }
    }

    pub fn on_signed_in(&mut self, result: Result<(String, Vec<String>), String>) {
        if let Some(form) = &mut self.settings {
            match result {
                Ok((account, models)) => {
                    form.available = models.clone();
                    form.status = Status::Ok(format!("Signed in as {account}"));
                    form.signed_in = Some(account);
                    if let Some((first, rest)) = models.split_first() {
                        if form.translate.text.trim().is_empty() {
                            form.translate = field(first);
                        }
                        if form.others.text.trim().is_empty() {
                            form.others = field(&rest.join(", "));
                        }
                    }
                }
                Err(m) => form.status = Status::Failed(m),
            }
        }
    }

    pub fn open_url(&mut self, tx: &mut Tx, url: &str) {
        if tx.open_url(url).is_err() {
            self.settings_failed(format!("Could not open the browser. Open this link: {url}"));
        }
    }
}

fn test_call(endpoint: &Endpoint, secrets: Arc<dyn Secrets>) -> Result<String, String> {
    let connection = cipher_llm::connect(endpoint, secrets).map_err(|e| e.to_string())?;
    let started = Instant::now();
    let done = connection
        .translate
        .complete(
            &Request {
                instructions: "This is a connection test. Reply with the single word OK.".into(),
                input: "ping".into(),
                max_output: 512,
                reasoning: endpoint.reasoning,
            },
            &mut |_| {},
        )
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{} replied “{}” in {:.1} s · {} tokens in, {} out",
        endpoint.translate_model,
        done.text.trim().chars().take(20).collect::<String>(),
        started.elapsed().as_secs_f32(),
        done.usage.input,
        done.usage.output,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> Settings {
        let mut s = Settings::blank();
        s.id = "e1".into();
        s.effort = Effort::Low;
        s
    }

    #[test]
    fn an_api_key_endpoint_needs_a_url_and_a_model_and_is_named_after_its_host() {
        let mut s = blank();
        assert!(s.endpoint().is_err());
        s.translate = field("model-a");
        assert_eq!(s.endpoint().unwrap_err(), "Give the base URL.");
        s.others = field("model-b, , model-a ,model-c");
        s.base_url = field(" https://api.example.com/v1 ");
        let e = s.endpoint().unwrap();
        assert_eq!(e.name, "api.example.com");
        assert_eq!(e.base_url, "https://api.example.com/v1");
        assert_eq!(e.small_model, None);
        assert_eq!(e.models, ["model-b", "model-c"]);
        assert_eq!(e.reasoning, Effort::Low);
        assert!(s.ready());
    }

    #[test]
    fn a_chatgpt_endpoint_is_ready_only_once_signed_in() {
        let mut s = blank();
        s.kind = Kind::ChatGpt;
        s.translate = field("model-b");
        s.small = field("model-c");
        let e = s.endpoint().unwrap();
        assert_eq!(
            (e.protocol, e.name.as_str()),
            (Protocol::ChatGpt, "ChatGPT")
        );
        assert_eq!(e.small_model.as_deref(), Some("model-c"));
        assert!(!s.ready());
        s.signed_in = Some("a@example.com".into());
        assert!(s.ready());
    }
}
