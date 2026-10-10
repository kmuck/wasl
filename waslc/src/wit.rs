//! WIT generation for the package (§ 13.1).
//!
//! The WIT is produced as text, then parsed by `wit-parser`: this yields a validated
//! representation, used both to build components and for display.

use std::fmt::Write;

use wit_component::WitPrinter;
use wit_parser::{PackageId, Resolve, WorldId};

use crate::diag::{Code, Diagnostic, Result};
use crate::hir::Program;
use crate::package::Package;

/// Converts a WaSL identifier into a WIT identifier: `fib_rec` → `fib-rec` (§ 13.2).
pub fn kebab(name: &str) -> String {
    name.replace('_', "-")
}

pub(crate) fn internal(e: anyhow::Error) -> Diagnostic {
    Diagnostic::without_span(Code::Internal, format!("internal error: {e:#}"))
}

/// WIT of the package, parsed by `wit-parser`.
pub struct Wit {
    pub resolve: Resolve,
    pub package: PackageId,
}

impl Wit {
    pub fn world(&self, name: &str) -> Result<WorldId> {
        self.resolve
            .select_world(&[self.package], Some(&kebab(name)))
            .map_err(internal)
    }

    /// Canonical name of each exported interface (e.g. `example:calc/math@0.1`), indexed by
    /// HIR interface id. It prefixes the exports of the core module (§ 13.3).
    pub fn canonical_interface_names(&self, program: &Program) -> Vec<Option<String>> {
        let package = &self.resolve.packages[self.package];
        program
            .interfaces
            .iter()
            .map(|iface| {
                let id = *package.interfaces.get(&kebab(&iface.name))?;
                self.resolve.canonicalized_id_of(id)
            })
            .collect()
    }

    /// Human-readable WIT text.
    pub fn to_text(&self) -> Result<String> {
        let mut printer = WitPrinter::default();
        printer
            .print(&self.resolve, self.package, &[])
            .map_err(internal)?;
        Ok(printer.output.to_string())
    }
}

/// Produces the WIT: interfaces exported by at least one world, then the worlds.
/// Internal interfaces and globals do not appear in it.
pub fn generate(program: &Program, package: &Package) -> Result<Wit> {
    // Every identifier is prefixed with `%`, which avoids any clash with a WIT keyword.
    let mut text = String::new();
    writeln!(
        text,
        "package {}:{}@{};",
        package.namespace, package.name, package.version
    )
    .unwrap();

    for (id, iface) in program.interfaces.iter().enumerate() {
        if !program.is_exported(id) {
            continue;
        }
        writeln!(text, "\ninterface %{} {{", kebab(&iface.name)).unwrap();
        for &f in &iface.funcs {
            let func = &program.funcs[f];
            let params: Vec<String> = func
                .params
                .iter()
                .map(|(name, ty)| format!("%{}: {ty}", kebab(name)))
                .collect();
            let ret = func.ret.map(|t| format!(" -> {t}")).unwrap_or_default();
            writeln!(
                text,
                "  %{}: func({}){ret};",
                kebab(&func.name),
                params.join(", ")
            )
            .unwrap();
        }
        writeln!(text, "}}").unwrap();
    }

    for world in &program.worlds {
        writeln!(text, "\nworld %{} {{", kebab(&world.name)).unwrap();
        for &iface in &world.exports {
            writeln!(
                text,
                "  export %{};",
                kebab(&program.interfaces[iface].name)
            )
            .unwrap();
        }
        writeln!(text, "}}").unwrap();
    }

    let mut resolve = Resolve::default();
    let package = resolve.push_str("wasl.wit", &text).map_err(internal)?;
    Ok(Wit { resolve, package })
}
