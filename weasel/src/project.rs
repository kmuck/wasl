//! A WaSL project on disk: `wasl.toml` and the `.wasl` files under `src/`.

use std::fs;
use std::path::{Path, PathBuf};

use waslc::{Analysis, Package, SourceMap};

use crate::error::{Error, Result};
use crate::manifest;

/// Directory where components are written.
pub const BUILD_DIR: &str = "build";

pub struct Project {
    pub package: Package,
    pub sources: SourceMap,
}

impl Project {
    /// Loads the project whose root is the current directory.
    pub fn load() -> Result<Project> {
        let manifest_text = match fs::read_to_string(manifest::FILE_NAME) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::new(format!(
                    "could not find `{}` in the current directory: weasel must be run from the root of a WaSL project",
                    manifest::FILE_NAME
                )));
            }
            Err(e) => {
                return Err(Error::new(format!(
                    "cannot read `{}`: {e}",
                    manifest::FILE_NAME
                )));
            }
        };
        let package = manifest::parse(&manifest_text)?;

        if !Path::new("src").is_dir() {
            return Err(Error::new(format!(
                "could not find the `src` directory next to `{}`",
                manifest::FILE_NAME
            )));
        }
        let mut files = Vec::new();
        collect_wasl_files(Path::new("src"), &mut files)?;
        // Sorted paths: the order of the files decides which error is reported first.
        files.sort();

        let mut sources = SourceMap::default();
        for path in files {
            let bytes =
                fs::read(&path).map_err(|e| Error::new(format!("cannot read `{path}`: {e}")))?;
            let text = String::from_utf8(bytes)
                .map_err(|_| Error::new(format!("`{path}` is not valid UTF-8")))?;
            sources.add(PathBuf::from(path), text);
        }
        Ok(Project { package, sources })
    }

    /// Analyzes the whole project.
    pub fn analyze(&self) -> Result<Analysis> {
        waslc::analyze(&self.package, &self.sources)
            .map_err(|d| Error::diagnostic(&d, &self.sources))
    }

    /// Builds the component of one world.
    pub fn build(&self, analysis: &Analysis, world: &str) -> Result<Vec<u8>> {
        analysis
            .build(world)
            .map_err(|d| Error::diagnostic(&d, &self.sources))
    }
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
