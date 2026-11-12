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
pub mod deps;
pub mod diag;
pub mod hir;
pub mod lexer;
pub mod package;
pub mod parser;
pub mod sema;
pub mod source;
pub mod types;
pub mod wit;

pub use deps::Dependencies;
pub use diag::{Diagnostic, Result};
pub use package::Package;
pub use source::SourceMap;
/// The WIT parser used by `waslc`, to parse directories of `.wit` files for
/// [`Dependencies::add_resolve`].
pub use wit_parser;

use diag::Code;

/// A world of the package, with WaSL names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    pub name: String,
    /// Interfaces exported by the world, in declaration order.
    pub exports: Vec<ExportedInterface>,
    /// Interfaces imported by the world, in declaration order, with their WIT name:
    /// `accounts`, `wasi:cli/stdout@0.2.6`.
    pub imports: Vec<String>,
    /// Interfaces of other packages that the world exports with an implementation
    /// (§ 2.8), in declaration order, with their WIT name: `wasi:cli/run@0.3.0`.
    pub implements: Vec<String>,
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
    interface_names: Vec<Option<wit::InterfaceName>>,
    worlds: Vec<World>,
}

/// Analyzes a package: checks every source file and prepares the WIT of the package.
///
/// Returns every error found, in the order of § 11.2: one per file with a syntax error;
/// otherwise the first error of the declarations, or one per function body with an
/// error. Sources must be given in a deterministic order (by convention, sorted by
/// path): that order decides the order of the errors. `deps` holds the WIT of the packages it
/// depends on (§ 3.5).
pub fn analyze(
    package: &Package,
    sources: &SourceMap,
    deps: &Dependencies,
) -> std::result::Result<Analysis, Vec<Diagnostic>> {
    let own = format!("{}:{}", package.namespace, package.name);
    if deps.names().any(|name| name == own) {
        return Err(vec![Diagnostic::without_span(
            Code::Manifest,
            format!("the package `{own}` cannot depend on itself"),
        )]);
    }
    // Each file is parsed even when another one has a syntax error, but a package with
    // a syntax error is not analyzed: its missing declarations would cause false errors.
    if deps.names().any(|name| name == "wasl:std") {
        return Err(vec![Diagnostic::without_span(
            Code::Manifest,
            "`wasl:std` is in `[dependencies]`, but every project depends on the standard library without declaring it: remove it",
        )]);
    }
    // The files of the package, then those of the standard library (§ 3.5).
    let mut items = Vec::new();
    let mut errors = Vec::new();
    let files = sources
        .ids()
        .map(|id| (id, sources.get(id)))
        .chain(source::library().iter().map(|(id, file)| (*id, file)));
    for (id, file) in files {
        match lexer::tokenize(id, &file.text).and_then(parser::parse) {
            Ok(file) => items.extend(file),
            Err(d) => errors.push(d),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let program = sema::analyze(&items, deps, &mut errors).map_err(|d| vec![d])?;
    if !errors.is_empty() {
        return Err(errors);
    }
    let wit = wit::generate(&program, package, deps).map_err(|d| vec![d])?;
    let interface_names = wit.interface_names(&program);
    let worlds = program
        .worlds
        .iter()
        .map(|world| World {
            name: world.name.clone(),
            // Implementations of interfaces of other packages are not called by name.
            exports: world
                .exports
                .iter()
                .filter(|&&iface| program.interfaces[iface].implements.is_none())
                .map(|&iface| ExportedInterface {
                    name: program.interfaces[iface].name.clone(),
                    functions: program.interfaces[iface]
                        .funcs
                        .iter()
                        .map(|&f| program.funcs[f].name.clone())
                        .collect(),
                })
                .collect(),
            imports: world
                .imports
                .iter()
                .map(|&iface| match &program.interfaces[iface].foreign {
                    Some(f) => f.wit_name(),
                    None => wit::kebab(&program.interfaces[iface].name),
                })
                .collect(),
            implements: world
                .exports
                .iter()
                .filter_map(|&iface| program.interfaces[iface].implements)
                .filter_map(|foreign| program.interfaces[foreign].foreign.as_ref())
                .map(|f| f.wit_name())
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

    /// The parameters of the function `function` of interface `interface`, with their
    /// types as written in that interface: `[("inv", "invoice")]`.
    pub fn params(&self, interface: &str, function: &str) -> Option<Vec<(String, String)>> {
        let program = &self.program;
        let iface = program
            .interfaces
            .iter()
            .position(|i| i.name == interface)?;
        let func = program.interfaces[iface]
            .funcs
            .iter()
            .map(|&f| &program.funcs[f])
            .find(|f| f.name == function)?;
        Some(
            func.params
                .iter()
                .map(|(name, ty)| (name.clone(), program.types.display_in(*ty, Some(iface))))
                .collect(),
        )
    }

    /// WIT of the package: its exported interfaces and its worlds.
    pub fn wit(&self) -> Result<String> {
        self.wit.to_text()
    }

    /// The world `world`, or E9001 when the package does not define it.
    pub fn world(&self, world: &str) -> Result<&World> {
        self.worlds.iter().find(|w| w.name == world).ok_or_else(|| {
            let names: Vec<String> = self
                .worlds
                .iter()
                .map(|w| format!("`{}`", w.name))
                .collect();
            Diagnostic::without_span(
                Code::UnknownWorld,
                if names.is_empty() {
                    format!("unknown world `{world}`: the package does not define any world")
                } else {
                    format!(
                        "unknown world `{world}`: the package defines {}",
                        names.join(", ")
                    )
                },
            )
        })
    }

    /// Builds the component of the world `world`.
    pub fn build(&self, world: &str) -> Result<Vec<u8>> {
        self.world(world)?;
        let id = self
            .program
            .worlds
            .iter()
            .position(|w| w.name == world)
            .expect("the worlds of the analysis are those of the program");
        let module = codegen::generate(&self.program, id, &self.interface_names)?;
        component::build(module, &self.wit, world)
    }
}
