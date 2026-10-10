//! Errors reported to the user.

use std::fmt;

use waslc::{Diagnostic, SourceMap};

/// An error, already rendered as the text printed on stderr.
#[derive(Debug)]
pub struct Error(String);

impl Error {
    /// A general error: `error: <message>`.
    pub fn new(message: impl fmt::Display) -> Error {
        Error(format!("error: {message}"))
    }

    /// A compiler diagnostic, rendered with its location in the sources.
    pub fn diagnostic(diagnostic: &Diagnostic, sources: &SourceMap) -> Error {
        Error(diagnostic.display(sources).to_string())
    }

    /// An error already rendered by the caller.
    pub fn rendered(text: String) -> Error {
        Error(text)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
