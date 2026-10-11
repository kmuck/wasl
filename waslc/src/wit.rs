//! WIT generation for the package (§ 13.1).
//!
//! The WIT is produced as text, then parsed by `wit-parser`: this yields a validated
//! representation, used both to build components and for display.

use std::collections::HashMap;
use std::fmt::Write;

use wit_component::WitPrinter;
use wit_parser::{PackageId, Resolve, WorldId};

use crate::diag::{Code, Diagnostic, Result};
use crate::hir::{InterfaceId, Program};
use crate::package::Package;
use crate::types::{DeclId, DeclKind, Type, TypeKind, Types};

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
        // WIT elaboration adds `import types;` to a world for the types-only interfaces it
        // uses; WaSL worlds never import, so these lines are not written (§ v2.7.1).
        let mut text = String::new();
        let mut lines = printer
            .output
            .to_string()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into_iter()
            .peekable();
        while let Some(line) = lines.next() {
            if line.starts_with("  import ") {
                if lines.peek().is_some_and(|l| l.is_empty()) {
                    lines.next();
                }
                continue;
            }
            text.push_str(&line);
            text.push('\n');
        }
        Ok(text)
    }
}

/// Produces the WIT: the interfaces that appear in it (§ v2.7.1), then the worlds.
/// Internal interfaces, their types and globals do not appear in it.
pub fn generate(program: &Program, package: &Package) -> Result<Wit> {
    // Every identifier is prefixed with `%`, which avoids any clash with a WIT keyword.
    let types = &program.types;
    let mut text = String::new();
    writeln!(
        text,
        "package {}:{}@{};",
        package.namespace, package.name, package.version
    )
    .unwrap();

    let appears = appears(types, program);
    for (id, iface) in program.interfaces.iter().enumerate() {
        if !appears[id] {
            continue;
        }
        let exported = program.is_exported(id);
        writeln!(text, "\ninterface %{} {{", kebab(&iface.name)).unwrap();

        // Types of other interfaces: `use other.{a, b as other-b};`. A name is renamed
        // when it collides with a type of this interface or with another used type.
        let mut names: HashMap<DeclId, String> = HashMap::new();
        let mut taken: Vec<String> = iface
            .types
            .iter()
            .map(|&d| kebab(&types.decls[d].name))
            .collect();
        for &d in &iface.types {
            names.insert(d, kebab(&types.decls[d].name));
        }
        let foreign = foreign_decls(types, program, id, exported);
        let mut uses: Vec<(usize, Vec<String>)> = Vec::new();
        for d in foreign {
            let decl = &types.decls[d];
            let name = kebab(&decl.name);
            let local = if taken.contains(&name) {
                format!("{}-{name}", kebab(&program.interfaces[decl.interface].name))
            } else {
                name.clone()
            };
            let item = if local == name {
                format!("%{name}")
            } else {
                format!("%{name} as %{local}")
            };
            taken.push(local.clone());
            names.insert(d, local);
            match uses.iter_mut().find(|(i, _)| *i == decl.interface) {
                Some((_, items)) => items.push(item),
                None => uses.push((decl.interface, vec![item])),
            }
        }
        uses.sort_by_key(|(i, _)| *i);
        for (other, items) in &uses {
            writeln!(
                text,
                "  use %{}.{{{}}};",
                kebab(&program.interfaces[*other].name),
                items.join(", ")
            )
            .unwrap();
        }

        for &d in &iface.types {
            let decl = &types.decls[d];
            let name = kebab(&decl.name);
            match &decl.kind {
                DeclKind::Record(fields) => {
                    writeln!(text, "  record %{name} {{").unwrap();
                    for f in fields {
                        writeln!(
                            text,
                            "    %{}: {},",
                            kebab(&f.name),
                            wit_type(types, f.ty, &names)
                        )
                        .unwrap();
                    }
                }
                DeclKind::Variant(cases) => {
                    writeln!(text, "  variant %{name} {{").unwrap();
                    for c in cases {
                        match c.payload {
                            Some(p) => writeln!(
                                text,
                                "    %{}({}),",
                                kebab(&c.name),
                                wit_type(types, p, &names)
                            ),
                            None => writeln!(text, "    %{},", kebab(&c.name)),
                        }
                        .unwrap();
                    }
                }
                DeclKind::Enum(cases) | DeclKind::Flags(cases) => {
                    writeln!(text, "  {} %{name} {{", decl.kind.keyword()).unwrap();
                    for c in cases {
                        writeln!(text, "    %{},", kebab(c)).unwrap();
                    }
                }
            }
            writeln!(text, "  }}").unwrap();
        }

        if exported {
            for &f in &iface.funcs {
                let func = &program.funcs[f];
                let params: Vec<String> = func
                    .params
                    .iter()
                    .map(|(name, ty)| format!("%{}: {}", kebab(name), wit_type(types, *ty, &names)))
                    .collect();
                let ret = func
                    .ret
                    .map(|t| format!(" -> {}", wit_type(types, t, &names)))
                    .unwrap_or_default();
                writeln!(
                    text,
                    "  %{}: func({}){ret};",
                    kebab(&func.name),
                    params.join(", ")
                )
                .unwrap();
            }
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

/// Which interfaces appear in the WIT (§ v2.7.1): those exported by a world, and those
/// whose types an interface that appears uses.
pub fn appears(types: &Types, program: &Program) -> Vec<bool> {
    let mut appears: Vec<bool> = (0..program.interfaces.len())
        .map(|i| program.is_exported(i))
        .collect();
    let mut work: Vec<InterfaceId> = (0..appears.len()).filter(|&i| appears[i]).collect();
    while let Some(i) = work.pop() {
        for d in foreign_decls(types, program, i, program.is_exported(i)) {
            let other = types.decls[d].interface;
            if !appears[other] {
                appears[other] = true;
                work.push(other);
            }
        }
    }
    appears
}

/// A cycle of `use` between the interfaces that appear in the WIT, which WIT forbids
/// (E0213): each interface of the cycle with a type it uses from the next one. The cycle
/// starts with its first interface in declaration order.
pub fn use_cycle(types: &Types, program: &Program) -> Option<Vec<(InterfaceId, DeclId)>> {
    let appears = appears(types, program);
    // Edges: for each interface, the first type it uses from each other interface.
    let edges: Vec<Vec<(InterfaceId, DeclId)>> = (0..program.interfaces.len())
        .map(|i| {
            let mut out: Vec<(InterfaceId, DeclId)> = Vec::new();
            if appears[i] {
                for d in foreign_decls(types, program, i, program.is_exported(i)) {
                    let other = types.decls[d].interface;
                    if !out.iter().any(|&(o, _)| o == other) {
                        out.push((other, d));
                    }
                }
            }
            out
        })
        .collect();
    // Depth-first search; `state`: 0 not visited, 1 on the current path, 2 done.
    fn visit(
        i: InterfaceId,
        edges: &[Vec<(InterfaceId, DeclId)>],
        state: &mut [u8],
        path: &mut Vec<(InterfaceId, DeclId)>,
    ) -> Option<Vec<(InterfaceId, DeclId)>> {
        state[i] = 1;
        for &(other, d) in &edges[i] {
            path.push((i, d));
            if state[other] == 1 {
                let start = path.iter().position(|&(j, _)| j == other).unwrap();
                let mut cycle = path[start..].to_vec();
                let first = (0..cycle.len()).min_by_key(|&k| cycle[k].0).unwrap();
                cycle.rotate_left(first);
                return Some(cycle);
            }
            if state[other] == 0
                && let Some(cycle) = visit(other, edges, state, path)
            {
                return Some(cycle);
            }
            path.pop();
        }
        state[i] = 2;
        None
    }
    let mut state = vec![0u8; edges.len()];
    (0..edges.len()).find_map(|i| {
        if state[i] == 0 {
            visit(i, &edges, &mut state, &mut Vec::new())
        } else {
            None
        }
    })
}

/// The types of other interfaces that the WIT of interface `id` names: in its types and,
/// if `with_funcs`, in the signatures of its functions. In a deterministic order.
fn foreign_decls(
    types: &Types,
    program: &Program,
    id: InterfaceId,
    with_funcs: bool,
) -> Vec<DeclId> {
    let iface = &program.interfaces[id];
    let mut roots: Vec<Type> = iface
        .types
        .iter()
        .flat_map(|&d| types.children(types.decl_type_of(d)))
        .collect();
    if with_funcs {
        for &f in &iface.funcs {
            let func = &program.funcs[f];
            roots.extend(func.params.iter().map(|(_, t)| *t));
            roots.extend(func.ret);
        }
    }
    let mut found = Vec::new();
    for root in roots {
        named_decls(types, root, &mut found);
    }
    found.retain(|&d| types.decls[d].interface != id);
    found.sort_by_key(|&d| (types.decls[d].interface, d));
    found.dedup();
    found
}

/// The declared types written in a type expression, without looking inside them.
fn named_decls(types: &Types, ty: Type, found: &mut Vec<DeclId>) {
    match types.kind(ty) {
        TypeKind::Decl(d) => found.push(*d),
        _ => {
            for child in types.children(ty) {
                named_decls(types, child, found);
            }
        }
    }
}

/// WIT text of a type; `names` gives the name of each declared type in the interface.
fn wit_type(types: &Types, ty: Type, names: &HashMap<DeclId, String>) -> String {
    match types.kind(ty) {
        TypeKind::Prim(p) => p.name().to_string(),
        TypeKind::Char => "char".into(),
        TypeKind::String => "string".into(),
        // `mut` is erased (§ v2.7.1).
        TypeKind::List { elem, .. } => format!("list<{}>", wit_type(types, *elem, names)),
        TypeKind::Tuple(elems) => format!(
            "tuple<{}>",
            elems
                .iter()
                .map(|&t| wit_type(types, t, names))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeKind::Option(inner) => format!("option<{}>", wit_type(types, *inner, names)),
        TypeKind::Result { ok, err } => match (ok, err) {
            (None, None) => "result".into(),
            (Some(ok), None) => format!("result<{}>", wit_type(types, *ok, names)),
            (None, Some(err)) => format!("result<_, {}>", wit_type(types, *err, names)),
            (Some(ok), Some(err)) => format!(
                "result<{}, {}>",
                wit_type(types, *ok, names),
                wit_type(types, *err, names)
            ),
        },
        TypeKind::Decl(d) => format!("%{}", names[d]),
    }
}
