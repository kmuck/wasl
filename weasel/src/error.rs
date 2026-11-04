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

    /// The error of a manifest, located in the project whose root is `root`: the
    /// `wasl.toml` at the start of the message becomes `<root>/wasl.toml`.
    pub fn in_file(self, root: &std::path::Path) -> Error {
        if root.as_os_str().is_empty() {
            return self;
        }
        match self.0.strip_prefix("wasl.toml") {
            Some(rest) => Error(format!(
                "{}{rest}",
                crate::project::display(&root.join("wasl.toml"))
            )),
            None => self,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
