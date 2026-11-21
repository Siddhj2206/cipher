use std::collections::{BTreeSet, HashSet};

use crate::read::read_terms;
use crate::{Book, Error, Result, Term, TermId, TermStatus};

/// The exact Terms reviewed before a glossary-only merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermMerge {
    pub survivor: Term,
    pub duplicate: Term,
}

impl TermMerge {
    pub(crate) fn read(
        conn: &rusqlite::Connection,
        survivor: TermId,
        duplicate: TermId,
    ) -> Result<Self> {
        if survivor == duplicate {
            return Err(Error::Invalid("Choose two different Terms".into()));
        }
        let terms = read_terms(conn)?;
        let get = |id| {
            terms
                .iter()
                .find(|t| t.id == id && t.status != TermStatus::Dismissed)
                .cloned()
                .ok_or(Error::NotFound("active term", id.0))
        };
        Ok(Self {
            survivor: get(survivor)?,
            duplicate: get(duplicate)?,
        })
    }

    /// Source forms to keep in addition to the survivor's primary form.
    pub fn aliases(&self) -> Vec<&str> {
        self.survivor
            .source_forms()
            .chain(self.duplicate.source_forms())
            .filter(|s| !s.trim().is_empty() && *s != self.survivor.fields.source)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Both lists plus the losing rendering, without duplicates or the chosen rendering.
    pub fn forbidden(&self) -> Vec<&str> {
        let key = |s: &str| s.trim().to_lowercase().replace('’', "'");
        let mut seen = HashSet::from([key(&self.survivor.fields.rendering)]);
        let mut out: Vec<_> = self
            .survivor
            .forbidden
            .iter()
            .chain(&self.duplicate.forbidden)
            .chain(std::iter::once(&self.duplicate.fields.rendering))
            .filter(|s| !s.trim().is_empty() && seen.insert(key(s)))
            .map(String::as_str)
            .collect();
        out.sort_unstable();
        out
    }
}

impl Book {
    pub fn preview_term_merge(&self, survivor: TermId, duplicate: TermId) -> Result<TermMerge> {
        TermMerge::read(&self.conn, survivor, duplicate)
    }
}
