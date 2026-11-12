//! Source files and positions within them.

use std::path::PathBuf;

/// Identifier of a file in a [`SourceMap`], or of a file of the standard library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

/// The first id of the files of the standard library (§ 3.5): they are not in the
/// [`SourceMap`] of the package, and every map shows them.
const LIBRARY: u32 = 1 << 31;

/// The bit set in the ids of the test files of the package (§ 3.6).
const TEST: u32 = 1 << 30;

impl FileId {
    /// Whether the file belongs to the standard library `wasl:std`.
    pub fn is_library(self) -> bool {
        self.0 >= LIBRARY
    }

    /// Whether the file is a test file of the package, from `tests/` (§ 3.6).
    pub fn is_test(self) -> bool {
        !self.is_library() && self.0 & TEST != 0
    }
}

/// The source files of `wasl:std` (chapter 17), shipped inside the compiler.
const STD: &[(&str, &str)] = &[("wasl:std/text.wasl", include_str!("../std/text.wasl"))];

/// The files of the standard library, with their ids.
pub fn library() -> &'static [(FileId, SourceFile)] {
    static FILES: std::sync::OnceLock<Vec<(FileId, SourceFile)>> = std::sync::OnceLock::new();
    FILES.get_or_init(|| {
        STD.iter()
            .enumerate()
            .map(|(k, (path, text))| {
                (
                    FileId(LIBRARY + k as u32),
                    SourceFile::new(path.into(), text.to_string()),
                )
            })
            .collect()
    })
}

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
    /// The id of each file, in the order they were added.
    ids: Vec<FileId>,
}

impl SourceMap {
    /// Adds a source file of the package, from `src/` (§ 3.3).
    pub fn add(&mut self, path: PathBuf, text: String) -> FileId {
        self.push(path, text, 0)
    }

    /// Adds a test file of the package, from `tests/` (§ 3.6). [`crate::analyze`]
    /// analyzes it with the package, and [`crate::Analysis::build_tests`] builds its tests.
    pub fn add_test(&mut self, path: PathBuf, text: String) -> FileId {
        self.push(path, text, TEST)
    }

    fn push(&mut self, path: PathBuf, text: String, bit: u32) -> FileId {
        self.files.push(SourceFile::new(path, text));
        let id = FileId((self.files.len() as u32 - 1) | bit);
        self.ids.push(id);
        id
    }

    pub fn get(&self, id: FileId) -> &SourceFile {
        if id.is_library() {
            return &library()[(id.0 - LIBRARY) as usize].1;
        }
        &self.files[(id.0 & !TEST) as usize]
    }

    /// The ids of the files, in the order they were added.
    pub fn ids(&self) -> impl Iterator<Item = FileId> + use<> {
        self.ids.clone().into_iter()
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
