use std::path::PathBuf;

use thiserror::Error;

/// A chive runtime error. The variant maps to an [exit code][`exit_code`].
///
/// The distinction between kinds matters: a failed shell command is a *result*
/// the caller may already have captured, while a catalog that cannot be read
/// is a hard error the whole command must abort for.
#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error in {path}: {detail}")]
    Parse { path: PathBuf, detail: String },

    #[error("catalog is invalid: {0}")]
    Catalog(String),

    #[error("no catalog found; run `chive scan` first, or `chive import` a catalog")]
    MissingCatalog,

    #[error("refused: {0}")]
    Refused(String),

    #[error("command failed with exit code {code}: {command}")]
    Command { code: i32, command: String },
}

impl Error {
    /// The process exit code for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Refused(_) => 3,
            Error::MissingCatalog => 4,
            _ => 1,
        }
    }

    /// Helper to build an invocation-only parse error shorthand.
    pub fn parse(path: PathBuf, detail: impl Into<String>) -> Self {
        Error::Parse {
            path,
            detail: detail.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
