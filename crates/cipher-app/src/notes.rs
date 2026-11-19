//! Editable Book guidance and continuity notes.

use cipher_book::{
    AddressFields, AddressId, Book, Change, ChapterId, FormOfAddress, JobKind, JobSpec, JobState,
    RevisionKind, StoryNote, StoryNoteId,
};
use rune::TextEdit;

use crate::state::{Screen, State};

pub struct AddressEdit {
    pub id: Option<AddressId>,
    pub fields: [TextEdit; 4],
}

impl AddressEdit {
    fn new(id: Option<AddressId>, f: AddressFields) -> Self {
        Self {
            id,
            fields: [f.speaker, f.listener, f.source_form, f.rendering].map(|s| TextEdit::new(&s)),
        }
    }

    fn value(&self) -> AddressFields {
        AddressFields {
            speaker: self.fields[0].text.clone(),
            listener: self.fields[1].text.clone(),
            source_form: self.fields[2].text.clone(),
            rendering: self.fields[3].text.clone(),
        }
    }
}

pub struct NoteEdit {
    pub id: Option<StoryNoteId>,
    pub chapter: Option<ChapterId>,
    pub origin: String,
    pub text: TextEdit,
}

pub struct NotesEdit {
    pub forms: Vec<AddressEdit>,
    pub notes: Vec<NoteEdit>,
    saved_forms: Vec<FormOfAddress>,
    saved_notes: Vec<StoryNote>,
    saved_origins: Vec<String>,
}

impl NotesEdit {
    pub fn load(book: &Book) -> cipher_book::Result<Self> {
        let saved_forms = book.glossary()?.forms_of_address;
        let saved_notes = book.story_notes()?;
        let chapters = book.chapters()?;
        let forms = saved_forms
            .iter()
            .map(|f| AddressEdit::new(Some(f.id), f.fields.clone()))
            .collect();
        let notes: Vec<NoteEdit> = saved_notes
            .iter()
            .map(|n| {
                let origin = n
                    .chapter
                    .and_then(|id| chapters.iter().enumerate().find(|(_, c)| c.id == id))
                    .map_or_else(
                        || "Summary".into(),
                        |(i, c)| {
                            let label = format!("Chapter {}", c.number.unwrap_or(i as i64 + 1));
                            c.title
                                .as_ref()
                                .filter(|t| !t.is_empty())
                                .map_or(label.clone(), |t| format!("{label} · {t}"))
                        },
                    );
                NoteEdit {
                    id: Some(n.id),
                    chapter: n.chapter,
                    origin,
                    text: TextEdit::new(&n.text),
                }
            })
            .collect();
        let saved_origins = notes.iter().map(|n| n.origin.clone()).collect();
        Ok(Self {
            forms,
            notes,
            saved_forms,
            saved_notes,
            saved_origins,
        })
    }

    pub fn add_form(&mut self) {
        self.forms
            .push(AddressEdit::new(None, AddressFields::default()));
    }

    pub fn changed(&self) -> bool {
        self.forms.len() != self.saved_forms.len()
            || self.notes.len() != self.saved_notes.len()
            || self
                .forms
                .iter()
                .zip(&self.saved_forms)
                .any(|(a, b)| a.id != Some(b.id) || a.value() != b.fields)
            || self
                .notes
                .iter()
                .zip(&self.saved_notes)
                .any(|(a, b)| a.id != Some(b.id) || a.text.text != b.text || a.chapter != b.chapter)
    }

    pub fn revert(&mut self) {
        self.forms = self
            .saved_forms
            .iter()
            .map(|f| AddressEdit::new(Some(f.id), f.fields.clone()))
            .collect();
        self.notes = self
            .saved_notes
            .iter()
            .zip(&self.saved_origins)
            .map(|(n, origin)| NoteEdit {
                id: Some(n.id),
                chapter: n.chapter,
                origin: origin.clone(),
                text: TextEdit::new(&n.text),
            })
            .collect();
    }

    pub fn changes(&self) -> Result<Option<Change>, String> {
        let mut changes = Vec::new();
        let mut forms = Vec::new();
        for (i, row) in self.forms.iter().enumerate() {
            let f = row.value();
            if [&f.speaker, &f.listener, &f.source_form, &f.rendering]
                .iter()
                .all(|v| v.trim().is_empty())
            {
                continue;
            }
            if [&f.speaker, &f.listener, &f.rendering]
                .iter()
                .any(|v| v.trim().is_empty())
            {
                return Err(format!(
                    "Form {} needs a speaker, listener and rendering.",
                    i + 1
                ));
            }
            forms.push((row.id, f));
        }
        for old in &self.saved_forms {
            match forms.iter().find(|(id, _)| *id == Some(old.id)) {
                None => changes.push(Change::EditFormOfAddress {
                    id: old.id,
                    fields: None,
                }),
                Some((_, f)) if *f != old.fields => changes.push(Change::EditFormOfAddress {
                    id: old.id,
                    fields: Some(f.clone()),
                }),
                _ => {}
            }
        }
        for (id, f) in forms {
            if id.is_none() {
                changes.push(Change::AddFormOfAddress(f));
            }
        }
        for old in &self.saved_notes {
            match self.notes.iter().find(|n| n.id == Some(old.id)) {
                None => changes.push(Change::EditStoryNote {
                    id: old.id,
                    text: None,
                }),
                Some(n) if n.text.text.trim().is_empty() => changes.push(Change::EditStoryNote {
                    id: old.id,
                    text: None,
                }),
                Some(n) if n.text.text != old.text => changes.push(Change::EditStoryNote {
                    id: old.id,
                    text: Some(n.text.text.clone()),
                }),
                _ => {}
            }
        }
        for n in &self.notes {
            if n.id.is_none() && !n.text.text.trim().is_empty() {
                changes.push(Change::AddStoryNote {
                    chapter: n.chapter,
                    text: n.text.text.clone(),
                });
            }
        }
        Ok((!changes.is_empty()).then_some(Change::Group {
            kind: RevisionKind::Notes,
            label: "Edited Book notes".into(),
            changes,
        }))
    }

    fn save(&mut self, book: &mut Book) -> Result<bool, String> {
        if self.saved_forms != book.glossary().map_err(|e| e.to_string())?.forms_of_address
            || self.saved_notes != book.story_notes().map_err(|e| e.to_string())?
        {
            return Err(
                "Book notes changed since you opened them — Revert to load the latest".into(),
            );
        }
        let change = self.changes()?;
        let changed = change.is_some();
        if let Some(change) = change {
            book.apply(change).map_err(|e| e.to_string())?;
        }
        *self = Self::load(book).map_err(|e| e.to_string())?;
        Ok(changed)
    }
}

impl State {
    pub fn open_notes(&mut self) {
        if (self.screen == Screen::Draft && !self.lib.chapters.is_empty())
            || self.fix.review.is_some()
        {
            return;
        }
        if !self.notes.as_ref().is_some_and(NotesEdit::changed) {
            match NotesEdit::load(self.lib.book()) {
                Ok(edit) => self.notes = Some(edit),
                Err(e) => {
                    self.toast = Some(format!("Could not read Book notes: {e}"));
                    return;
                }
            }
        }
        self.palette = None;
        self.screen = Screen::Notes;
    }

    pub fn save_notes(&mut self) {
        if self.notes_compacting() {
            self.toast =
                Some("Wait for Story Notes compaction to finish before saving Book notes.".into());
            return;
        }
        let Some(edit) = &mut self.notes else { return };
        self.toast = Some(match edit.save(self.lib.book_mut()) {
            Ok(true) => "Book notes saved: future translations follow them.".into(),
            Ok(false) => "Nothing changed.".into(),
            Err(e) => format!("Could not save Book notes: {e}"),
        });
    }

    pub fn revert_notes(&mut self) {
        if let Some(edit) = &mut self.notes {
            match NotesEdit::load(self.lib.book()) {
                Ok(latest) => {
                    if edit.saved_forms == latest.saved_forms
                        && edit.saved_notes == latest.saved_notes
                    {
                        edit.revert();
                    } else {
                        *edit = latest;
                    }
                }
                Err(e) => self.toast = Some(format!("Could not read Book notes: {e}")),
            }
        }
    }

    pub fn on_notes_job_finished(&mut self, job: cipher_book::JobId) {
        if self
            .lib
            .book()
            .job(job)
            .is_ok_and(|j| j.kind == JobKind::CompactNotes)
            && self.notes.as_ref().is_some_and(|edit| !edit.changed())
        {
            self.revert_notes();
        }
    }

    pub fn notes_compacting(&self) -> bool {
        self.lib.book().jobs(true).map_or(true, |jobs| {
            jobs.iter().any(|j| {
                j.kind == JobKind::CompactNotes
                    && matches!(
                        j.state,
                        JobState::Queued | JobState::Running | JobState::Paused
                    )
            })
        })
    }

    pub fn compact_notes(&mut self) {
        if self.notes.as_ref().is_none_or(|e| e.notes.is_empty()) || self.notes_compacting() {
            return;
        }
        if self.notes.as_ref().is_some_and(NotesEdit::changed) {
            self.toast = Some("Save or revert your Book notes before compacting.".into());
            return;
        }
        self.toast = Some(
            match self.lib.book_mut().enqueue(JobSpec {
                kind: JobKind::CompactNotes,
                parent: None,
                chapter: None,
                suggestion: None,
                input: None,
            }) {
                Ok(_) => "Story Notes compaction queued.".into(),
                Err(e) => format!("Could not queue compaction: {e}"),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Library;
    use cipher_book::BookMeta;

    fn fixture() -> (tempfile::TempDir, Book) {
        let dir = tempfile::tempdir().unwrap();
        let mut book = Book::create(
            dir.path().join("notes.cipher"),
            BookMeta {
                title: "Notes test".into(),
                source_lang: "ko".into(),
                target_lang: "en".into(),
            },
        )
        .unwrap();
        for name in ["A", "B"] {
            book.apply(Change::AddFormOfAddress(fields(name))).unwrap();
            book.apply(Change::AddStoryNote {
                chapter: None,
                text: name.into(),
            })
            .unwrap();
        }
        (dir, book)
    }

    fn fields(speaker: &str) -> AddressFields {
        AddressFields {
            speaker: speaker.into(),
            listener: "Listener".into(),
            source_form: "형".into(),
            rendering: "Brother".into(),
        }
    }

    fn edit_all(edit: &mut NotesEdit) {
        edit.forms[0].fields[0].text = "Edited".into();
        edit.forms.remove(1);
        edit.forms.push(AddressEdit::new(None, fields("New")));
        edit.notes[0].text.text = "Edited note".into();
        edit.notes.remove(1);
        edit.notes.push(NoteEdit {
            id: None,
            chapter: None,
            origin: "Summary".into(),
            text: TextEdit::new("New note"),
        });
    }

    #[test]
    fn diff_groups_add_edit_delete_for_both_sections() {
        let (_dir, book) = fixture();
        let mut edit = NotesEdit::load(&book).unwrap();
        assert!(!edit.changed());
        assert_eq!(edit.changes().unwrap(), None);
        let forms = edit.saved_forms.clone();
        let notes = edit.saved_notes.clone();
        edit_all(&mut edit);
        assert_eq!(
            edit.changes().unwrap(),
            Some(Change::Group {
                kind: RevisionKind::Notes,
                label: "Edited Book notes".into(),
                changes: vec![
                    Change::EditFormOfAddress {
                        id: forms[0].id,
                        fields: Some(fields("Edited"))
                    },
                    Change::EditFormOfAddress {
                        id: forms[1].id,
                        fields: None
                    },
                    Change::AddFormOfAddress(fields("New")),
                    Change::EditStoryNote {
                        id: notes[0].id,
                        text: Some("Edited note".into())
                    },
                    Change::EditStoryNote {
                        id: notes[1].id,
                        text: None
                    },
                    Change::AddStoryNote {
                        chapter: None,
                        text: "New note".into()
                    },
                ],
            })
        );
    }

    #[test]
    fn empty_book_opens_notes_and_escape_returns_to_its_draft() {
        let (_dir, book) = fixture();
        let mut state = State::new(Library::from_book(book).unwrap());
        assert_eq!(state.screen, Screen::Draft);
        state.open_notes();
        assert_eq!(state.screen, Screen::Notes);
        assert_eq!(state.notes.as_ref().unwrap().notes.len(), 2);
        let mut runtime = rune::Runtime::new(
            state,
            crate::view::view,
            (1280.0, 900.0),
            rune::Env::new(false, 1.0),
        );
        runtime.on_key = Some(crate::state::on_key);
        runtime.frame(1.0 / 60.0);
        runtime.key(rune::KeyEvent {
            key: rune::Key::Escape,
            mods: Default::default(),
        });
        runtime.frame(1.0 / 60.0);
        assert_eq!(runtime.state.screen, Screen::Draft);
        assert!(runtime.state.draft.as_ref().unwrap().text.text.is_empty());
    }

    #[test]
    fn state_save_persists_one_revision_and_reloads() {
        let (_dir, book) = fixture();
        let path = book.path().to_path_buf();
        let before = book.revisions(100).unwrap().len();
        drop(book);
        let mut s = State::new(Library::open(&path).unwrap());
        s.open_notes();
        edit_all(s.notes.as_mut().unwrap());
        s.save_notes();
        assert!(!s.notes.as_ref().unwrap().changed());
        let fresh = Book::open(&path).unwrap();
        assert_eq!(
            fresh
                .glossary()
                .unwrap()
                .forms_of_address
                .into_iter()
                .map(|f| f.fields)
                .collect::<Vec<_>>(),
            vec![fields("Edited"), fields("New")]
        );
        assert_eq!(
            fresh
                .story_notes()
                .unwrap()
                .into_iter()
                .map(|n| n.text)
                .collect::<Vec<_>>(),
            vec!["Edited note", "New note"]
        );
        assert_eq!(fresh.revisions(100).unwrap().len(), before + 1);
    }

    #[test]
    fn revert_restores_removed_rows_and_labels() {
        let (_dir, mut book) = fixture();
        let chapter = ChapterId(
            book.apply(Change::AddChapter {
                number: Some(7),
                title: Some("Tower".into()),
                source: vec!["Source".into()],
            })
            .unwrap()
            .created[0],
        );
        book.apply(Change::AddStoryNote {
            chapter: Some(chapter),
            text: "Chapter note".into(),
        })
        .unwrap();
        let mut edit = NotesEdit::load(&book).unwrap();
        edit_all(&mut edit);
        edit.notes.clear();
        edit.revert();
        assert!(!edit.changed());
        assert_eq!(edit.notes[2].origin, "Chapter 7 · Tower");
    }

    #[test]
    fn empty_rows_drop_but_incomplete_rows_reject_without_writes() {
        let (_dir, mut book) = fixture();
        let mut edit = NotesEdit::load(&book).unwrap();
        edit.add_form();
        assert!(edit.changed());
        assert_eq!(edit.changes().unwrap(), None);
        assert!(!edit.save(&mut book).unwrap());
        assert_eq!(edit.forms.len(), 2);
        edit.add_form();
        edit.forms[2].fields[2].text = "source only".into();
        assert!(
            edit.save(&mut book)
                .unwrap_err()
                .contains("speaker, listener and rendering")
        );
        assert_eq!(book.glossary().unwrap().forms_of_address.len(), 2);
        edit.forms.pop();
        edit.forms[0].fields = std::array::from_fn(|_| TextEdit::new(" "));
        edit.save(&mut book).unwrap();
        assert_eq!(book.glossary().unwrap().forms_of_address.len(), 1);
        let deleted = edit.notes[0].id.unwrap();
        edit.notes[0].text.text = " \n ".into();
        edit.notes.push(NoteEdit {
            id: None,
            chapter: None,
            origin: "Summary".into(),
            text: TextEdit::new(" \n"),
        });
        assert_eq!(
            edit.changes().unwrap(),
            Some(Change::Group {
                kind: RevisionKind::Notes,
                label: "Edited Book notes".into(),
                changes: vec![Change::EditStoryNote {
                    id: deleted,
                    text: None
                }],
            })
        );
        edit.save(&mut book).unwrap();
        assert_eq!(book.story_notes().unwrap().len(), 1);
    }

    #[test]
    fn opening_reloads_clean_state_and_preserves_dirty_state() {
        let (_dir, mut book) = fixture();
        book.apply(Change::AddChapter {
            number: Some(1),
            title: None,
            source: vec!["Source".into()],
        })
        .unwrap();
        let mut s = State::new(Library::open(book.path()).unwrap());
        s.screen = Screen::Draft;
        s.open_notes();
        assert!(s.notes.is_none());
        s.screen = Screen::Reading;
        s.open_notes();
        s.lib
            .book_mut()
            .apply(Change::AddStoryNote {
                chapter: None,
                text: "External".into(),
            })
            .unwrap();
        s.open_notes();
        assert_eq!(s.notes.as_ref().unwrap().notes.len(), 3);
        s.notes.as_mut().unwrap().notes[0].text.text = "Unsaved".into();
        s.screen = Screen::Reading;
        s.open_notes();
        assert_eq!(s.notes.as_ref().unwrap().notes[0].text.text, "Unsaved");
    }

    #[test]
    fn stale_compaction_snapshot_cannot_overwrite_summary() {
        let (_dir, book) = fixture();
        let mut s = State::new(Library::open(book.path()).unwrap());
        s.open_notes();
        s.notes.as_mut().unwrap().notes[0].text.text = "Unsaved".into();
        s.lib
            .book_mut()
            .apply(Change::CompactStoryNotes {
                summary: "New summary".into(),
            })
            .unwrap();
        let revisions = s.lib.book().revisions(100).unwrap().len();
        s.save_notes();
        assert!(
            s.toast
                .as_ref()
                .unwrap()
                .contains("Revert to load the latest")
        );
        assert_eq!(s.lib.book().revisions(100).unwrap().len(), revisions);
        assert_eq!(s.lib.book().story_notes().unwrap()[0].text, "New summary");
        s.revert_notes();
        assert_eq!(s.notes.as_ref().unwrap().notes[0].text.text, "New summary");
        assert!(!s.notes.as_ref().unwrap().changed());
    }

    #[test]
    fn compaction_completion_reloads_only_clean_editors() {
        for dirty in [false, true] {
            let (_dir, book) = fixture();
            let mut s = State::new(Library::open(book.path()).unwrap());
            s.open_notes();
            s.compact_notes();
            let mut job = s.lib.book_mut().claim_next_job().unwrap().unwrap();
            if dirty {
                s.notes.as_mut().unwrap().notes[0].text.text = "Unsaved".into();
            }
            s.lib
                .book_mut()
                .apply(Change::CompactStoryNotes {
                    summary: "New summary".into(),
                })
                .unwrap();
            job.state = JobState::Done;
            s.lib.book_mut().save_job(&job).unwrap();
            s.on_notes_job_finished(job.id);
            let edit = s.notes.as_ref().unwrap();
            assert_eq!(
                edit.notes[0].text.text,
                if dirty { "Unsaved" } else { "New summary" }
            );
            assert_eq!(edit.changed(), dirty);
        }
    }

    #[test]
    fn saves_during_active_compaction_do_not_write() {
        for state in [JobState::Queued, JobState::Running, JobState::Paused] {
            let (_dir, book) = fixture();
            let mut s = State::new(Library::open(book.path()).unwrap());
            s.open_notes();
            s.compact_notes();
            let mut job = s.lib.book().jobs(true).unwrap().remove(0);
            job.state = state;
            s.lib.book_mut().save_job(&job).unwrap();
            let notes = s.lib.book().story_notes().unwrap();
            let forms = s.lib.book().glossary().unwrap().forms_of_address;
            let revisions = s.lib.book().revisions(100).unwrap().len();
            edit_all(s.notes.as_mut().unwrap());
            s.save_notes();
            assert!(s.toast.as_ref().unwrap().contains("compaction to finish"));
            assert_eq!(s.lib.book().story_notes().unwrap(), notes);
            assert_eq!(s.lib.book().glossary().unwrap().forms_of_address, forms);
            assert_eq!(s.lib.book().revisions(100).unwrap().len(), revisions);
            assert!(s.notes.as_ref().unwrap().changed());
        }
    }

    #[test]
    fn compaction_queues_once_and_empty_notes_do_not_queue() {
        let (_dir, book) = fixture();
        let mut s = State::new(Library::open(book.path()).unwrap());
        s.open_notes();
        s.notes.as_mut().unwrap().notes[0].text.text = "Unsaved".into();
        s.compact_notes();
        assert!(s.lib.book().jobs(true).unwrap().is_empty());
        s.revert_notes();
        s.compact_notes();
        s.compact_notes();
        assert_eq!(s.lib.book().jobs(true).unwrap().len(), 1);
        assert!(s.notes_compacting());
        let (_empty_dir, mut empty) = fixture();
        for n in empty.story_notes().unwrap() {
            empty
                .apply(Change::EditStoryNote {
                    id: n.id,
                    text: None,
                })
                .unwrap();
        }
        let mut s = State::new(Library::open(empty.path()).unwrap());
        s.open_notes();
        s.compact_notes();
        assert!(s.lib.book().jobs(true).unwrap().is_empty());
    }
}
