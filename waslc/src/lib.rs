//! Compiler library for the WaSL language.
//!
//! `waslc` turns WaSL source texts into WebAssembly components. It does not touch the
//! filesystem: the caller (typically the `weasel` tool) loads the project and writes the
//! results.
//!
//! Compilation happens in two steps:
//!
//! 1. [`analyze`] checks the whole package (lexer → parser → sema) and prepares its WIT;
//! 2. [`Analysis::build`] produces the component of one world (codegen → component).
//!
//! A world is an entry point: each world is built separately into its own component.

pub mod ast;
pub mod codegen;
pub mod component;
pub mod diag;
pub mod hir;
pub mod lexer;
pub mod package;
pub mod parser;
pub mod sema;
pub mod source;
pub mod types;
pub mod wit;

pub use diag::{Diagnostic, Result};
pub use package::Package;
pub use source::SourceMap;

use diag::Code;

/// A world of the package, with WaSL names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    pub name: String,
    /// Interfaces exported by the world, in declaration order.
    pub exports: Vec<ExportedInterface>,
}

/// An interface exported by a world, with WaSL names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedInterface {
    pub name: String,
    pub functions: Vec<String>,
}

/// A package that passed semantic analysis, ready to be built world by world.
pub struct Analysis {
    program: hir::Program,
    wit: wit::Wit,
    interface_names: Vec<Option<String>>,
    worlds: Vec<World>,
}

/// Analyzes a package: checks every source file and prepares the WIT of the package.
///
/// Sources must be given in a deterministic order (by convention, sorted by path): that
/// order decides which error is reported first.
pub fn analyze(package: &Package, sources: &SourceMap) -> Result<Analysis> {
    let mut items = Vec::new();
    for id in sources.ids() {
        let tokens = lexer::tokenize(id, &sources.get(id).text)?;
        items.extend(parser::parse(tokens)?);
    }
    let program = sema::analyze(&items)?;
    let wit = wit::generate(&program, package)?;
    let interface_names = wit.canonical_interface_names(&program);
    let worlds = program
        .worlds
        .iter()
        .map(|world| World {
            name: world.name.clone(),
            exports: world
                .exports
                .iter()
                .map(|&iface| ExportedInterface {
                    name: program.interfaces[iface].name.clone(),
                    functions: program.interfaces[iface]
                        .funcs
                        .iter()
                        .map(|&f| program.funcs[f].name.clone())
                        .collect(),
                })
                .collect(),
        })
        .collect();
    Ok(Analysis {
        program,
        wit,
        interface_names,
        worlds,
    })
}

impl Analysis {
    /// The worlds of the package, in declaration order.
    pub fn worlds(&self) -> &[World] {
        &self.worlds
    }

    /// WIT of the package: its exported interfaces and its worlds.
    pub fn wit(&self) -> Result<String> {
        self.wit.to_text()
    }

    /// Builds the component of the world `world`.
    pub fn build(&self, world: &str) -> Result<Vec<u8>> {
        let Some(id) = self.program.worlds.iter().position(|w| w.name == world) else {
            let names: Vec<String> = self
                .worlds
                .iter()
                .map(|w| format!("`{}`", w.name))
                .collect();
            return Err(Diagnostic::without_span(
                Code::UnknownWorld,
                if names.is_empty() {
                    format!("unknown world `{world}`: the package does not define any world")
                } else {
                    format!(
                        "unknown world `{world}`: the package defines {}",
                        names.join(", ")
                    )
                },
            ));
        };
        let module = codegen::generate(&self.program, id, &self.interface_names)?;
        component::build(module, &self.wit, world)
    }
}
