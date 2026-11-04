//! Resolution of the dependencies of a project (specification § 3.5).
//!
//! - A path to a WaSL project is loaded and analyzed: its WIT is given to `waslc`.
//! - A path to a directory of `.wit` files is parsed with `wit-parser`.
//! - A version is fetched from a registry with the crates of `wkg`, and pinned in
//!   `wkg.lock` at the root of the project.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use waslc::Dependencies;
use waslc::wit_parser::Resolve;

use crate::error::{Error, Result};
use crate::manifest::{self, Source};
use crate::project::{Project, display};

/// The file where `wkg` records the exact versions and digests of registry packages.
pub const LOCK_FILE: &str = "wkg.lock";

/// Resolves every dependency of `project`, fetching the registry packages if needed.
pub fn resolve(project: &Project) -> Result<Dependencies> {
    let mut graph = Graph::default();
    graph.walk(project, &mut Vec::new())?;
    let fetched = if graph.registry.is_empty() {
        BTreeMap::new()
    } else {
        registry::fetch(&project.root, &graph.registry)?
    };
    let mut cx = Build {
        fetched,
        projects: graph.projects,
        built: BTreeMap::new(),
    };
    cx.deps_of(project)
}

/// A manifest error, located in the `wasl.toml` of `project`.
fn manifest_error(project: &Project, message: impl std::fmt::Display) -> Error {
    Error::rendered(format!(
        "{}: error[E0200]: {message}",
        display(&project.root.join(manifest::FILE_NAME))
    ))
}

/// Where a path dependency points to.
fn path_of(project: &Project, path: &Path) -> PathBuf {
    project.root.join(path)
}

/// The WaSL projects reachable through path dependencies, and the registry packages
/// that any of them requires.
#[derive(Default)]
struct Graph {
    /// Registry packages and their version.
    registry: BTreeMap<String, String>,
    /// WaSL projects of path dependencies, by canonical path.
    projects: BTreeMap<PathBuf, Project>,
}

impl Graph {
    fn walk(&mut self, project: &Project, stack: &mut Vec<(PathBuf, String)>) -> Result<()> {
        stack.push((canonical(&project.root)?, project.name()));
        for dep in &project.manifest.dependencies {
            match &dep.source {
                Source::Registry(version) => {
                    if let Some(other) = self.registry.get(&dep.name)
                        && other != version
                    {
                        return Err(manifest_error(
                            project,
                            format!(
                                "dependency `{}` is required at two versions, `{other}` and `{version}`",
                                dep.name
                            ),
                        ));
                    }
                    self.registry.insert(dep.name.clone(), version.clone());
                }
                Source::Path(path) => {
                    let dir = path_of(project, path);
                    if !dir.join(manifest::FILE_NAME).is_file() {
                        continue;
                    }
                    let key = canonical(&dir)?;
                    if let Some(start) = stack.iter().position(|(p, _)| *p == key) {
                        let mut cycle: Vec<String> = stack[start..]
                            .iter()
                            .map(|(_, n)| format!("`{n}`"))
                            .collect();
                        cycle.push(format!("`{}`", dep.name));
                        return Err(manifest_error(
                            project,
                            format!("dependency cycle: {}", cycle.join(" → ")),
                        ));
                    }
                    if !self.projects.contains_key(&key) {
                        let sub = Project::load_at(&dir)?;
                        if sub.name() != dep.name {
                            return Err(manifest_error(
                                project,
                                format!(
                                    "dependency `{}` points to `{}`, whose package is `{}`",
                                    dep.name,
                                    display(&dir),
                                    sub.name()
                                ),
                            ));
                        }
                        self.walk(&sub, stack)?;
                        self.projects.insert(key, sub);
                    }
                }
            }
        }
        stack.pop();
        Ok(())
    }
}

fn canonical(path: &Path) -> Result<PathBuf> {
    let path = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    fs::canonicalize(path).map_err(|e| Error::new(format!("cannot read `{}`: {e}", display(path))))
}

struct Build {
    fetched: BTreeMap<String, Vec<u8>>,
    projects: BTreeMap<PathBuf, Project>,
    /// The dependencies and the WIT of each WaSL project already analyzed.
    built: BTreeMap<PathBuf, (Dependencies, String)>,
}

impl Build {
    /// The dependencies of `project`, with the dependencies of its WaSL dependencies.
    fn deps_of(&mut self, project: &Project) -> Result<Dependencies> {
        let mut deps = Dependencies::default();
        let located = |e: waslc::Diagnostic| manifest_error(project, e.message);
        for dep in &project.manifest.dependencies {
            match &dep.source {
                Source::Registry(version) => {
                    deps.add_wit_binary(&dep.name, version, &self.fetched[&dep.name])
                        .map_err(located)?;
                }
                Source::Path(path) => {
                    let dir = path_of(project, path);
                    if dir.join(manifest::FILE_NAME).is_file() {
                        let key = canonical(&dir)?;
                        let (sub_deps, wit) = self.wasl(&key)?;
                        deps.include(&sub_deps).map_err(located)?;
                        let wit_path = display(&dir.join(manifest::FILE_NAME));
                        deps.add_wit(&dep.name, &wit_path, &wit).map_err(located)?;
                    } else if dir.exists() {
                        let mut resolve = Resolve::default();
                        let (package, _) = resolve.push_path(&dir).map_err(|e| {
                            manifest_error(
                                project,
                                format!(
                                    "invalid WIT in dependency `{}` (`{}`): {e:#}",
                                    dep.name,
                                    display(&dir)
                                ),
                            )
                        })?;
                        deps.add_resolve(&dep.name, None, resolve, package)
                            .map_err(located)?;
                    } else {
                        return Err(manifest_error(
                            project,
                            format!(
                                "dependency `{}` points to `{}`, which does not exist",
                                dep.name,
                                display(&dir)
                            ),
                        ));
                    }
                }
            }
        }
        Ok(deps)
    }

    /// Analyzes the WaSL project at `key` and returns its dependencies and its WIT.
    fn wasl(&mut self, key: &Path) -> Result<(Dependencies, String)> {
        if let Some(built) = self.built.get(key) {
            return Ok(built.clone());
        }
        let project = self.projects.remove(key).expect("walked");
        let deps = self.deps_of(&project)?;
        let analysis = project.analyze_with(&deps)?;
        let wit = analysis
            .wit()
            .map_err(|d| Error::diagnostic(&d, &project.sources))?;
        self.built
            .insert(key.to_path_buf(), (deps.clone(), wit.clone()));
        Ok((deps, wit))
    }
}

/// Registry packages, fetched with the crates of `wkg`.
mod registry {
    use super::*;

    use tokio::io::AsyncReadExt;
    use wasm_pkg_client::caching::FileCache;
    use wasm_pkg_core::lock::LockFile;
    use wasm_pkg_core::resolver::{
        Dependency, DependencyResolution, DependencyResolver, RegistryPackage,
    };

    /// Fetches the packages `name → version`, from the lock file when it pins them, and
    /// updates the lock file.
    pub fn fetch(
        root: &Path,
        packages: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, Vec<u8>>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::new(format!("cannot start the registry client: {e}")))?;
        runtime
            .block_on(fetch_async(&root.join(LOCK_FILE), packages))
            .map_err(|e| Error::new(format!("cannot fetch the dependencies: {e:#}")))
    }

    async fn fetch_async(
        lock_path: &Path,
        packages: &BTreeMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, Vec<u8>>> {
        let config = wasm_pkg_client::Config::global_defaults().await?;
        let cache = FileCache::global_cache().await?;
        let mut lock = if lock_path.is_file() {
            LockFile::load_from_path(lock_path, false).await?
        } else {
            LockFile::new_with_path([], lock_path).await?
        };
        let map = {
            let mut resolver = DependencyResolver::new(Some(config), Some(&lock), cache)?;
            for (name, version) in packages {
                let dependency = Dependency::Package(RegistryPackage {
                    name: None,
                    version: format!("={version}").parse()?,
                    registry: None,
                });
                resolver.add_dependency(&name.parse()?, &dependency).await?;
            }
            resolver.resolve().await?
        };
        let mut fetched = BTreeMap::new();
        for (name, resolution) in map.iter() {
            let DependencyResolution::Registry(registry) = resolution else {
                unreachable!("only registry packages are given to the resolver")
            };
            let mut bytes = Vec::new();
            let mut reader = Box::pin(registry.fetch().await?);
            reader.read_to_end(&mut bytes).await?;
            fetched.insert(name.to_string(), bytes);
        }
        let before = (lock.version, lock.packages.clone());
        lock.update_dependencies(&map);
        if lock != before {
            lock.write().await?;
        }
        Ok(fetched)
    }
}
