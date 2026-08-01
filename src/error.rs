//! Structured error type for cipher.
//!
//! Codes E001–E007 are a public API contract — stable strings that are never
//! removed or repurposed across releases (only deprecated and replaced).

use serde_json::Error as JsonError;

pub type Result<T> = std::result::Result<T, Error>;

/// Structured error type for cipher.
///
/// | Code  | Domain                    | Exit code |
/// |-------|---------------------------|-----------|
/// | E001  | Configuration             | 2         |
/// | E002  | I/O                       | 3         |
/// | E003  | Profile                   | 5         |
/// | E004  | Glossary                  | 6         |
/// | E005  | Translation / Provider    | 4         |
/// | E006  | Validation                | 1         |
/// | E007  | State                     | 70        |
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// E001 — Configuration
    #[error("config error: {0}")]
    Config(String),

    /// E002 — I/O
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// E003 — Profile
    #[error("profile '{name}' not found")]
    ProfileNotFound { name: String },

    /// E004 — Glossary
    #[error("glossary error: {0}")]
    Glossary(String),

    /// E005 — Translation / Provider
    #[error("{kind} request failed: {detail}")]
    Provider { kind: String, detail: String },

    /// E006 — Validation (user error; displays bare, without a code prefix)
    #[error("{message}")]
    Validation { message: String },

    /// E007 — State / internal invariant
    #[error("state error: {0}")]
    State(String),
}

impl Clone for Error {
    fn clone(&self) -> Self {
        match self {
            Error::Config(s) => Error::Config(s.clone()),
            Error::Io(e) => Error::Io(std::io::Error::new(e.kind(), e.to_string())),
            Error::ProfileNotFound { name } => Error::ProfileNotFound { name: name.clone() },
            Error::Glossary(s) => Error::Glossary(s.clone()),
            Error::Provider { kind, detail } => Error::Provider {
                kind: kind.clone(),
                detail: detail.clone(),
            },
            Error::Validation { message } => Error::Validation {
                message: message.clone(),
            },
            Error::State(s) => Error::State(s.clone()),
        }
    }
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Error::Config(_) => "E001",
            Error::Io(_) => "E002",
            Error::ProfileNotFound { .. } => "E003",
            Error::Glossary(_) => "E004",
            Error::Provider { .. } => "E005",
            Error::Validation { .. } => "E006",
            Error::State(_) => "E007",
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Config(_) => 2,
            Error::Io(_) => 3,
            Error::ProfileNotFound { .. } => 5,
            Error::Glossary(_) => 6,
            Error::Provider { .. } => 4,
            Error::Validation { .. } => 1,
            Error::State(_) => 70,
        }
    }

    pub fn suggestion(&self) -> Option<&'static str> {
        match self {
            Error::Config(_) => Some("Run 'cipher doctor' for configuration diagnostics."),
            Error::Io(_) => Some("Check that the path exists and is readable and writable."),
            Error::ProfileNotFound { .. } => {
                Some("Run 'cipher profile list' to see available profiles.")
            }
            Error::Glossary(_) => Some("Check the glossary JSON format (book/glossary.json)."),
            Error::Provider { .. } => {
                Some("Run 'cipher profile test' to check your API key and model settings.")
            }
            Error::Validation { .. } => Some("Fix the reported issue and try again."),
            Error::State(_) => {
                Some("This looks like an internal error — rerun with --verbose and report it.")
            }
        }
    }

    /// Wrap an I/O error with context while preserving its `ErrorKind` (E002).
    pub(crate) fn io(context: impl std::fmt::Display, source: std::io::Error) -> Self {
        Error::Io(std::io::Error::new(
            source.kind(),
            format!("{context}: {source}"),
        ))
    }
}

impl From<JsonError> for Error {
    fn from(e: JsonError) -> Self {
        // JSON errors come from serializing our own plain structs, which
        // cannot fail in practice — treat as an internal state error.
        Error::State(format!("JSON error: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_errors() -> Vec<Error> {
        vec![
            Error::Config("broken config".into()),
            Error::Io(std::io::Error::other("disk failure")),
            Error::ProfileNotFound {
                name: "og".to_string(),
            },
            Error::Glossary("bad glossary json".into()),
            Error::Provider {
                kind: "gemini".to_string(),
                detail: "timeout".to_string(),
            },
            Error::Validation {
                message: "no chapters found".to_string(),
            },
            Error::State("corrupt state file".into()),
        ]
    }

    #[test]
    fn codes_follow_contract() {
        let expected = ["E001", "E002", "E003", "E004", "E005", "E006", "E007"];
        for (err, code) in sample_errors().into_iter().zip(expected) {
            assert_eq!(err.code(), code, "for {err:?}");
        }
    }

    #[test]
    fn exit_codes_follow_table() {
        let expected = [2, 3, 5, 6, 4, 1, 70];
        for (err, code) in sample_errors().into_iter().zip(expected) {
            assert_eq!(err.exit_code(), code, "for {err:?}");
        }
    }

    #[test]
    fn suggestions_cover_all_variants() {
        let expected = [true, true, true, true, true, true, true];
        for (err, has) in sample_errors().into_iter().zip(expected) {
            assert_eq!(err.suggestion().is_some(), has, "for {err:?}");
        }
    }

    #[test]
    fn display_matches_planned_wording() {
        assert_eq!(
            Error::ProfileNotFound { name: "og".into() }.to_string(),
            "profile 'og' not found"
        );
        assert_eq!(Error::Config("x".into()).to_string(), "config error: x");
        assert_eq!(
            Error::Io(std::io::Error::other("disk")).to_string(),
            "I/O error: disk"
        );
        assert_eq!(Error::Glossary("x".into()).to_string(), "glossary error: x");
        assert_eq!(
            Error::Provider {
                kind: "gemini".into(),
                detail: "timeout".into()
            }
            .to_string(),
            "gemini request failed: timeout"
        );
        assert_eq!(
            Error::Validation {
                message: "bad value".into()
            }
            .to_string(),
            "bad value"
        );
        assert_eq!(Error::State("x".into()).to_string(), "state error: x");
    }

    #[test]
    fn io_from_error_is_e002() {
        let err: Error = std::io::Error::new(std::io::ErrorKind::NotFound, "missing").into();
        assert_eq!(err.code(), "E002");
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn io_helper_preserves_kind_and_context() {
        let err = Error::io(
            "Failed to read foo.md",
            std::io::Error::from_raw_os_error(2),
        );
        assert_eq!(err.code(), "E002");
        assert_eq!(err.exit_code(), 3);
        assert!(
            err.to_string().contains("Failed to read foo.md"),
            "got: {err}"
        );
        let Error::Io(inner) = err else {
            panic!("expected Io variant");
        };
        assert_eq!(inner.kind(), std::io::ErrorKind::NotFound);
    }

    #[test]
    fn serde_json_error_converts_to_state() {
        let err: Error = serde_json::from_str::<serde_json::Value>("{")
            .unwrap_err()
            .into();
        assert_eq!(err.code(), "E007");
        assert_eq!(err.exit_code(), 70);
    }
}
