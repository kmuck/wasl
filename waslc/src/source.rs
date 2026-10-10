//! Source files and positions within them.

use std::path::PathBuf;

/// Identifier of a file in a [`SourceMap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

/// A byte range `[start, end)` in a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file: FileId, start: usize, end: usize) -> Self {
        Span {
            file,
            start: start as u32,
            end: end as u32,
        }
    }

    /// Range from the start of `self` to the end of `other`.
    pub fn to(self, other: Span) -> Span {
        Span {
            file: self.file,
            start: self.start,
            end: other.end,
        }
    }
}

/// A source file loaded in memory.
pub struct SourceFile {
    /// Path shown in diagnostics (relative to the project root).
    pub path: PathBuf,
    pub text: String,
    /// Byte offset of the start of each line.
    line_starts: Vec<usize>,
}

impl SourceFile {
    pub fn new(path: PathBuf, text: String) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        SourceFile {
            path,
            text,
            line_starts,
        }
    }

    /// Line and column of an offset (1-based; the column counts bytes).
    pub fn line_col(&self, offset: u32) -> (usize, usize) {
        let offset = offset as usize;
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        (line + 1, offset - self.line_starts[line] + 1)
    }
}

/// All the source files of a package.
#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn add(&mut self, path: PathBuf, text: String) -> FileId {
        self.files.push(SourceFile::new(path, text));
        FileId(self.files.len() as u32 - 1)
    }

    pub fn get(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    pub fn ids(&self) -> impl Iterator<Item = FileId> + use<> {
        (0..self.files.len() as u32).map(FileId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col() {
        let f = SourceFile::new("a.wasl".into(), "ab\ncd\n\nef".into());
        assert_eq!(f.line_col(0), (1, 1));
        assert_eq!(f.line_col(1), (1, 2));
        assert_eq!(f.line_col(3), (2, 1));
        assert_eq!(f.line_col(6), (3, 1));
        assert_eq!(f.line_col(8), (4, 2));
    }
}
