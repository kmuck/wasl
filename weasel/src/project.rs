//! A WaSL project on disk: `wasl.toml`, the `.wasl` files under `src/`, and the test
//! files under `tests/`.

use std::fs;
use std::path::{Path, PathBuf};

use waslc::{Analysis, Dependencies, Package, SourceMap};

use crate::deps;
use crate::error::{Error, Result};
use crate::manifest::{self, Manifest};

/// Number of compilation errors shown, like the Go compiler (§ 14.1).
pub const MAX_ERRORS: usize = 10;

/// Directory where components are written.
pub const BUILD_DIR: &str = "build";

pub struct Project {
    /// The root of the project, as given: empty for the current directory, the path of
    /// the dependency for a path dependency (`../catalog`).
    pub root: PathBuf,
    pub package: Package,
    pub manifest: Manifest,
    pub sources: SourceMap,
}

impl Project {
    /// Loads the project whose root is the current directory.
    pub fn load() -> Result<Project> {
        Project::load_at(Path::new(""))
    }

    /// Loads the project whose root is `root`. Its files are displayed with paths that
    /// start with `root`.
    pub fn load_at(root: &Path) -> Result<Project> {
        let manifest_path = root.join(manifest::FILE_NAME);
        let manifest_text = match fs::read_to_string(&manifest_path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && root.as_os_str().is_empty() => {
                return Err(Error::new(format!(
                    "could not find `{}` in the current directory: weasel must be run from the root of a WaSL project",
                    manifest::FILE_NAME
                )));
            }
            Err(e) => {
                return Err(Error::new(format!(
                    "cannot read `{}`: {e}",
                    display(&manifest_path)
                )));
            }
        };
        let manifest = manifest::parse(&manifest_text).map_err(|e| e.in_file(root))?;

        let src = root.join("src");
        if !src.is_dir() {
            return Err(Error::new(format!(
                "could not find the `src` directory next to `{}`",
                display(&manifest_path)
            )));
        }
        let mut files = Vec::new();
        collect_wasl_files(&src, &mut files)?;
        // Sorted paths: the order of the files decides which error is reported first.
        files.sort();

        let mut sources = SourceMap::default();
        for path in files {
            let text = read_source(&path)?;
            sources.add(PathBuf::from(path), text);
        }
        Ok(Project {
            root: root.to_path_buf(),
            package: manifest.package.clone(),
            manifest,
            sources,
        })
    }

    /// Adds the test files of `tests/` to the sources (§ 3.6), when the directory exists.
    pub fn add_tests(&mut self) -> Result<()> {
        let tests = self.root.join("tests");
        if !tests.is_dir() {
            return Ok(());
        }
        let mut files = Vec::new();
        collect_wasl_files(&tests, &mut files)?;
        files.sort();
        for path in files {
            let text = read_source(&path)?;
            self.sources.add_test(PathBuf::from(path), text);
        }
        Ok(())
    }

    /// The name of the package, `namespace:name`.
    pub fn name(&self) -> String {
        format!("{}:{}", self.package.namespace, self.package.name)
    }

    /// Resolves the dependencies of the project (§ 3.5), then analyzes it.
    pub fn analyze(&self) -> Result<Analysis> {
        let deps = deps::resolve(self)?;
        self.analyze_with(&deps)
    }

    /// Analyzes the whole project, with its dependencies already resolved. The error
    /// shows the first [`MAX_ERRORS`] errors of the compilation, one per line (§ 14.1).
    pub fn analyze_with(&self, deps: &Dependencies) -> Result<Analysis> {
        waslc::analyze(&self.package, &self.sources, deps).map_err(|errors| {
            let mut lines: Vec<String> = errors
                .iter()
                .take(MAX_ERRORS)
                .map(|d| {
                    if d.code.as_str() == "E0200" && d.span.is_none() {
                        // Errors of the dependencies are about the manifest (§ 11.2).
                        format!(
                            "{}: error[{}]: {}",
                            display(&self.root.join(manifest::FILE_NAME)),
                            d.code.as_str(),
                            d.message
                        )
                    } else {
                        d.display(&self.sources).to_string()
                    }
                })
                .collect();
            match errors.len().saturating_sub(MAX_ERRORS) {
                0 => {}
                1 => lines.push("error: 1 more error not shown".into()),
                n => lines.push(format!("error: {n} more errors not shown")),
            }
            Error::rendered(lines.join("\n"))
        })
    }

    /// Builds the component of one world.
    pub fn build(&self, analysis: &Analysis, world: &str) -> Result<Vec<u8>> {
        analysis
            .build(world)
            .map_err(|d| Error::diagnostic(&d, &self.sources))
    }
}

/// A path as displayed in messages, with `/` separators.
pub fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Reads a source file, which must be UTF-8 (§ 4.1).
fn read_source(path: &str) -> Result<String> {
    let bytes = fs::read(path).map_err(|e| Error::new(format!("cannot read `{path}`: {e}")))?;
    String::from_utf8(bytes).map_err(|_| Error::new(format!("`{path}` is not valid UTF-8")))
}

/// Collects the `.wasl` files of a directory, with `/`-separated paths.
fn collect_wasl_files(dir: &Path, out: &mut Vec<String>) -> Result<()> {
    let read_error =
        |e: std::io::Error| Error::new(format!("cannot read `{}`: {e}", dir.display()));
    for entry in fs::read_dir(dir).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        let path = entry.path();
        if path.is_dir() {
            collect_wasl_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "wasl") {
            out.push(path.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}
