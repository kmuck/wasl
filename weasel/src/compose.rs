//! Composition of a component with the components that provide its imports, for
//! `weasel call` (specification § 14.2), with `wac-graph`.
//!
//! - An interface of the package is provided by the first world, in declaration order,
//!   that exports it.
//! - An interface of a path dependency that is a WaSL project, by the first world of
//!   that project that exports it.
//! - The interfaces of WASI 0.2 that wasmtime implements are left to the host.
//!
//! Providers are composed in turn when they have imports of their own.

use wac_graph::{CompositionGraph, EncodeOptions, types::Package};
use waslc::Analysis;

use crate::error::{Error, Result};
use crate::manifest::{self, Source};
use crate::project::Project;

/// The packages of WASI 0.2 that the host provides (§ 14.2).
const HOST_PACKAGES: &[&str] = &[
    "wasi:cli",
    "wasi:io",
    "wasi:clocks",
    "wasi:random",
    "wasi:filesystem",
];

/// Whether the host provides an imported interface, `wasi:cli/stdout@0.2.6`.
fn from_host(import: &str) -> bool {
    HOST_PACKAGES.iter().any(|p| {
        import
            .strip_prefix(p)
            .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// The WIT name of an interface of `project` with its version, `example:catalog/carts@0.1.0`.
fn full_name(project: &Project, interface: &str) -> String {
    format!(
        "{}/{}@{}",
        project.name(),
        interface.replace('_', "-"),
        project.package.version
    )
}

/// The component of `world`, with every import that the project or its path
/// dependencies provide plugged in.
pub fn component(project: &Project, analysis: &Analysis, world: &str) -> Result<Vec<u8>> {
    compose(project, analysis, world, &mut Vec::new())
}

fn compose(
    project: &Project,
    analysis: &Analysis,
    world: &str,
    stack: &mut Vec<String>,
) -> Result<Vec<u8>> {
    let key = format!("{}/{world}", project.name());
    if stack.contains(&key) {
        stack.push(key);
        return Err(Error::new(format!(
            "the worlds import each other in a cycle: {}",
            stack.join(" → ")
        )));
    }
    let bytes = project.build(analysis, world)?;
    let def = analysis
        .worlds()
        .iter()
        .find(|w| w.name == world)
        .expect("world of the analysis");
    let imports: Vec<String> = def
        .imports
        .iter()
        .map(|i| {
            if i.contains(':') {
                i.clone()
            } else {
                full_name(project, i)
            }
        })
        .filter(|i| !from_host(i))
        .collect();
    if imports.is_empty() {
        return Ok(bytes);
    }

    stack.push(key);
    let deps = path_dependencies(project)?;
    let mut plugs: Vec<(String, Vec<u8>)> = Vec::new();
    for import in &imports {
        let provider = std::iter::once((project, analysis))
            .chain(deps.iter().map(|(p, a)| (p, a)))
            .find_map(|(p, a)| {
                a.worlds()
                    .iter()
                    .find(|w| w.exports.iter().any(|e| full_name(p, &e.name) == *import))
                    .map(|w| (p, a, w.name.clone()))
            });
        let Some((p, a, w)) = provider else {
            return Err(Error::new(format!(
                "no component provides `{import}`: add it as a path dependency on a WaSL project"
            )));
        };
        let name = format!("{}/{w}", p.name());
        if !plugs.iter().any(|(n, _)| *n == name) {
            let plug = compose(p, a, &w, stack)?;
            plugs.push((name, plug));
        }
    }
    stack.pop();

    let failed = |e: &dyn std::fmt::Display| {
        Error::new(format!(
            "cannot compose the component of world `{world}`: {e}"
        ))
    };
    let mut graph = CompositionGraph::new();
    let socket = Package::from_bytes(world, None, bytes, graph.types_mut())
        .map_err(|e| failed(&format!("{e:#}")))?;
    let socket = graph.register_package(socket).map_err(|e| failed(&e))?;
    let mut ids = Vec::new();
    for (k, (_, plug)) in plugs.into_iter().enumerate() {
        let package = Package::from_bytes(&format!("plug{k}"), None, plug, graph.types_mut())
            .map_err(|e| failed(&format!("{e:#}")))?;
        ids.push(graph.register_package(package).map_err(|e| failed(&e))?);
    }
    wac_graph::plug(&mut graph, ids, socket).map_err(|e| failed(&e))?;
    graph
        .encode(EncodeOptions::default())
        .map_err(|e| failed(&e))
}

/// The WaSL projects of the path dependencies of `project`, analyzed.
fn path_dependencies(project: &Project) -> Result<Vec<(Project, Analysis)>> {
    let mut out = Vec::new();
    for dep in &project.manifest.dependencies {
        if let Source::Path(path) = &dep.source {
            let dir = project.root.join(path);
            if dir.join(manifest::FILE_NAME).is_file() {
                let sub = Project::load_at(&dir)?;
                let analysis = sub.analyze()?;
                out.push((sub, analysis));
            }
        }
    }
    Ok(out)
}
