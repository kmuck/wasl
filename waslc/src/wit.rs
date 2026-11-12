//! WIT generation for the package (§ 13.1).
//!
//! The WIT is produced as text, then parsed by `wit-parser`: this yields a validated
//! representation, used both to build components and for display.

use std::collections::HashMap;
use std::fmt::Write;

use wit_component::WitPrinter;
use wit_parser::{PackageId, Resolve, WorldId};

use crate::deps::Dependencies;
use crate::diag::{Code, Diagnostic, Result};
use crate::hir::{Doc, FuncKind, InterfaceId, Program, Spelling};
use crate::package::Package;
use crate::types::{DeclId, DeclKind, Type, TypeKind, Types};

/// A type the WIT names: a declared type or an alias of the package (§ 7.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Named {
    Decl(DeclId),
    Alias(usize),
}

impl Named {
    /// The interface that declares it.
    pub fn interface(self, types: &Types) -> InterfaceId {
        match self {
            Named::Decl(d) => types.decls[d].interface,
            Named::Alias(a) => types.aliases[a].interface,
        }
    }

    /// Its name in code.
    pub fn name(self, types: &Types) -> &str {
        match self {
            Named::Decl(d) => &types.decls[d].name,
            Named::Alias(a) => &types.aliases[a].name,
        }
    }
}

/// Converts a WaSL identifier into a WIT identifier: `fib_rec` → `fib-rec` (§ 13.2).
pub fn kebab(name: &str) -> String {
    name.replace('_', "-")
}

pub(crate) fn internal(e: anyhow::Error) -> Diagnostic {
    Diagnostic::without_span(Code::Internal, format!("internal error: {e:#}"))
}

/// WIT of the package, parsed by `wit-parser`.
pub struct Wit {
    /// The package, with the packages of the dependencies.
    pub resolve: Resolve,
    pub package: PackageId,
    /// The imports written in each world, as `wit-component` prints them
    /// (`accounts`, `wasi:cli/stdout@0.2.6`).
    imports: HashMap<String, Vec<String>>,
}

/// The names of an interface in the core module (§ 13.3).
#[derive(Debug, Clone)]
pub struct InterfaceName {
    /// With the canonical version, for the standard names: `example:calc/math@0.1`.
    pub canonical: String,
    /// With the full version, for the names of async exports and of `task.return`
    /// (§ 13.13): `example:calc/math@0.1.0`.
    pub full: String,
}

impl Wit {
    pub fn world(&self, name: &str) -> Result<WorldId> {
        self.resolve
            .select_world(&[self.package], Some(&kebab(name)))
            .map_err(internal)
    }

    /// The names of each interface of the WIT, indexed by HIR interface id. They prefix
    /// the imports and exports of the core module (§ 13.3).
    pub fn interface_names(&self, program: &Program) -> Vec<Option<InterfaceName>> {
        let package = &self.resolve.packages[self.package];
        program
            .interfaces
            .iter()
            .map(|iface| {
                let foreign = match iface.implements {
                    Some(i) => &program.interfaces[i].foreign,
                    None => &iface.foreign,
                };
                let id = match foreign {
                    Some(f) => f.id,
                    None => *package.interfaces.get(&kebab(&iface.name))?,
                };
                Some(InterfaceName {
                    canonical: self.resolve.canonicalized_id_of(id)?,
                    full: self.resolve.id_of(id)?,
                })
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
        // uses; only the imports written in the world are kept (§ 13.1).
        let mut text = String::new();
        let mut world: Option<&Vec<String>> = None;
        let mut lines = printer
            .output
            .to_string()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into_iter()
            .peekable();
        while let Some(line) = lines.next() {
            if let Some(name) = line.strip_prefix("world ") {
                let name = name.trim_end_matches(" {").trim_start_matches('%');
                world = self.imports.get(name);
            }
            let written = |l: &str| {
                let name = l
                    .trim_start_matches("  import ")
                    .trim_end_matches(';')
                    .replace('%', "");
                world.is_some_and(|w| w.contains(&name))
            };
            if line.starts_with("  import ") && !written(&line) {
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

/// Produces the WIT: the interfaces that appear in it (§ 13.1), then the worlds.
/// Internal interfaces, their types and globals do not appear in it.
pub fn generate(program: &Program, package: &Package, deps: &Dependencies) -> Result<Wit> {
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
        if !appears[id] || iface.foreign.is_some() {
            continue;
        }
        let exported = program.has_wit_funcs(id);
        text.push('\n');
        doc(&mut text, program, Doc::Interface(id), "");
        writeln!(text, "interface %{} {{", kebab(&iface.name)).unwrap();

        // Types of other interfaces: `use other.{a, b as other-b};`. A name is renamed
        // when it collides with a type of this interface or with another used type.
        let mut names: HashMap<Named, String> = HashMap::new();
        let own = iface
            .types
            .iter()
            .map(|&d| Named::Decl(d))
            .chain(iface.aliases.iter().map(|&a| Named::Alias(a)));
        for n in own {
            names.insert(n, kebab(n.name(types)));
        }
        let mut taken: Vec<String> = names.values().cloned().collect();
        taken.sort();
        let foreign = foreign_decls(types, program, id, exported);
        let mut uses: Vec<(usize, Vec<String>)> = Vec::new();
        for d in foreign {
            let interface = d.interface(types);
            // The spelling of the dependency, which may have uppercase letters (§ 13.2).
            let name = match d {
                Named::Decl(d) => types.wit_name(d),
                Named::Alias(_) => kebab(d.name(types)),
            };
            let local = if taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
                format!(
                    "{}-{name}",
                    kebab(&program.interfaces[interface].name).replace(':', "-")
                )
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
            match uses.iter_mut().find(|(i, _)| *i == interface) {
                Some((_, items)) => items.push(item),
                None => uses.push((interface, vec![item])),
            }
        }
        uses.sort_by_key(|(i, _)| *i);
        for (other, items) in &uses {
            let path = match &program.interfaces[*other].foreign {
                Some(f) => f.wit_name(),
                None => format!("%{}", kebab(&program.interfaces[*other].name)),
            };
            writeln!(text, "  use {path}.{{{}}};", items.join(", ")).unwrap();
        }

        for &a in &iface.aliases {
            let alias = &types.aliases[a];
            if in_wit(types, alias.ty) {
                doc(&mut text, program, Doc::Alias(a), "  ");
                writeln!(
                    text,
                    "  type %{} = {};",
                    kebab(&alias.name),
                    wit_type(types, alias.spelled, &names)
                )
                .unwrap();
            }
        }

        for &d in &iface.types {
            let decl = &types.decls[d];
            let name = kebab(&decl.name);
            doc(&mut text, program, Doc::Decl(d), "  ");
            match &decl.kind {
                DeclKind::Record(fields) => {
                    writeln!(text, "  record %{name} {{").unwrap();
                    for (k, f) in fields.iter().enumerate() {
                        doc(&mut text, program, Doc::Member(d, k), "    ");
                        let ty = program.spelled(Spelling::Member(d, k), f.ty);
                        writeln!(
                            text,
                            "    %{}: {},",
                            kebab(&f.name),
                            wit_type(types, ty, &names)
                        )
                        .unwrap();
                    }
                }
                DeclKind::Variant(cases) => {
                    writeln!(text, "  variant %{name} {{").unwrap();
                    for (k, c) in cases.iter().enumerate() {
                        doc(&mut text, program, Doc::Member(d, k), "    ");
                        match c.payload {
                            Some(p) => writeln!(
                                text,
                                "    %{}({}),",
                                kebab(&c.name),
                                wit_type(types, program.spelled(Spelling::Member(d, k), p), &names)
                            ),
                            None => writeln!(text, "    %{},", kebab(&c.name)),
                        }
                        .unwrap();
                    }
                }
                DeclKind::Resource(def) => {
                    // The fields are private: only the functions appear (§ 13.1).
                    writeln!(text, "  resource %{name} {{").unwrap();
                    let funcs = def
                        .constructor
                        .iter()
                        .chain(def.methods.iter().map(|(_, f)| f))
                        .chain(def.statics.iter().map(|(_, f)| f));
                    let mut funcs: Vec<usize> = funcs.copied().collect();
                    funcs.sort();
                    for f in funcs {
                        let func = &program.funcs[f];
                        let skip = matches!(func.kind, FuncKind::Method(_)) as usize;
                        let params = wit_params(types, program, f, skip, &names);
                        let ret = wit_ret(types, program, f, &names);
                        let func_kw = if func.is_async { "async func" } else { "func" };
                        doc(&mut text, program, Doc::Func(f), "    ");
                        match func.kind {
                            FuncKind::Constructor(_) => {
                                writeln!(text, "    constructor({params});").unwrap()
                            }
                            FuncKind::Static(_) => writeln!(
                                text,
                                "    %{}: static {func_kw}({params}){ret};",
                                kebab(&func.name)
                            )
                            .unwrap(),
                            _ => writeln!(
                                text,
                                "    %{}: {func_kw}({params}){ret};",
                                kebab(&func.name)
                            )
                            .unwrap(),
                        }
                    }
                }
                DeclKind::Enum(cases) | DeclKind::Flags(cases) => {
                    writeln!(text, "  {} %{name} {{", decl.kind.keyword()).unwrap();
                    for (k, c) in cases.iter().enumerate() {
                        doc(&mut text, program, Doc::Member(d, k), "    ");
                        writeln!(text, "    %{},", kebab(c)).unwrap();
                    }
                }
            }
            writeln!(text, "  }}").unwrap();
        }

        if exported {
            for &f in &iface.funcs {
                let func = &program.funcs[f];
                if func.kind != FuncKind::Free {
                    continue;
                }
                let params = wit_params(types, program, f, 0, &names);
                let ret = wit_ret(types, program, f, &names);
                let func_kw = if func.is_async { "async func" } else { "func" };
                doc(&mut text, program, Doc::Func(f), "  ");
                writeln!(text, "  %{}: {func_kw}({params}){ret};", kebab(&func.name)).unwrap();
            }
        }
        writeln!(text, "}}").unwrap();
    }

    let mut imports = HashMap::new();
    for (w, world) in program.worlds.iter().enumerate() {
        text.push('\n');
        doc(&mut text, program, Doc::World(w), "");
        writeln!(text, "world %{} {{", kebab(&world.name)).unwrap();
        let mut written = Vec::new();
        for &(import, iface) in &world.items {
            let foreign = match program.interfaces[iface].implements {
                Some(i) => &program.interfaces[i].foreign,
                None => &program.interfaces[iface].foreign,
            };
            let (name, printed) = match foreign {
                Some(f) => (f.wit_name(), f.wit_name()),
                None => {
                    let name = kebab(&program.interfaces[iface].name);
                    (format!("%{name}"), name)
                }
            };
            let keyword = if import { "import" } else { "export" };
            writeln!(text, "  {keyword} {name};").unwrap();
            if import {
                written.push(printed);
            }
        }
        writeln!(text, "}}").unwrap();
        imports.insert(kebab(&world.name), written);
    }

    let mut resolve = deps.resolve().clone();
    let package = resolve.push_str("wasl.wit", &text).map_err(internal)?;
    Ok(Wit {
        resolve,
        package,
        imports,
    })
}

/// Writes the doc comment of `target`, if it has one, line by line (§ 4.2, § 13.1).
fn doc(text: &mut String, program: &Program, target: Doc, indent: &str) {
    if let Some(doc) = program.docs.get(&target) {
        for line in doc.split('\n') {
            writeln!(text, "{indent}///{line}").unwrap();
        }
    }
}

/// Which interfaces appear in the WIT (§ 13.1): those exported by a world, and those
/// whose types an interface that appears uses.
pub fn appears(types: &Types, program: &Program) -> Vec<bool> {
    let mut appears: Vec<bool> = (0..program.interfaces.len())
        .map(|i| program.has_wit_funcs(i))
        .collect();
    let mut work: Vec<InterfaceId> = (0..appears.len()).filter(|&i| appears[i]).collect();
    while let Some(i) = work.pop() {
        for d in foreign_decls(types, program, i, program.has_wit_funcs(i)) {
            let other = d.interface(types);
            // The interfaces of other packages are in the WIT of their package.
            if !appears[other] && program.interfaces[other].foreign.is_none() {
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
pub fn use_cycle(types: &Types, program: &Program) -> Option<Vec<(InterfaceId, Named)>> {
    let appears = appears(types, program);
    // Edges: for each interface, the first type it uses from each other interface.
    let edges: Vec<Vec<(InterfaceId, Named)>> = (0..program.interfaces.len())
        .map(|i| {
            let mut out: Vec<(InterfaceId, Named)> = Vec::new();
            if appears[i] {
                for d in foreign_decls(types, program, i, program.has_wit_funcs(i)) {
                    let other = d.interface(types);
                    if program.interfaces[other].foreign.is_none()
                        && !out.iter().any(|&(o, _)| o == other)
                    {
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
        edges: &[Vec<(InterfaceId, Named)>],
        state: &mut [u8],
        path: &mut Vec<(InterfaceId, Named)>,
    ) -> Option<Vec<(InterfaceId, Named)>> {
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

/// The types of other interfaces that the WIT of interface `id` names: in its types and
/// aliases and, if `with_funcs`, in the signatures of its functions. In a deterministic
/// order.
fn foreign_decls(
    types: &Types,
    program: &Program,
    id: InterfaceId,
    with_funcs: bool,
) -> Vec<Named> {
    let iface = &program.interfaces[id];
    let mut roots: Vec<Type> = Vec::new();
    for &d in &iface.types {
        let members: Vec<Option<Type>> = match &types.decls[d].kind {
            DeclKind::Record(fields) => fields.iter().map(|f| Some(f.ty)).collect(),
            DeclKind::Variant(cases) => cases.iter().map(|c| c.payload).collect(),
            // The fields of a resource are private: they never cross the boundary.
            DeclKind::Enum(_) | DeclKind::Flags(_) | DeclKind::Resource(_) => Vec::new(),
        };
        for (k, t) in members.into_iter().enumerate() {
            roots.extend(t.map(|t| program.spelled(Spelling::Member(d, k), t)));
        }
    }
    for &a in &iface.aliases {
        if in_wit(types, types.aliases[a].ty) {
            roots.push(types.aliases[a].spelled);
        }
    }
    if with_funcs {
        for &f in &iface.funcs {
            let func = &program.funcs[f];
            roots.extend(
                func.params
                    .iter()
                    .enumerate()
                    .map(|(k, (_, t))| program.spelled(Spelling::Param(f, k), *t)),
            );
            roots.extend(func.ret.map(|t| program.spelled(Spelling::Ret(f), t)));
        }
    }
    let mut found = Vec::new();
    for root in roots {
        named_types(types, root, &mut found);
    }
    found.retain(|d| d.interface(types) != id);
    found.sort_by_key(|&d| (d.interface(types), d));
    found.dedup();
    found
}

/// The declared types and aliases written in a type, without looking inside them.
fn named_types(types: &Types, ty: Type, found: &mut Vec<Named>) {
    match types.kind(ty) {
        TypeKind::Decl(d) => found.push(Named::Decl(*d)),
        TypeKind::Alias(a) => found.push(Named::Alias(*a)),
        _ => {
            for child in types.children(ty) {
                named_types(types, child, found);
            }
        }
    }
}

/// Whether the type an alias names can be written in the WIT (§ 7.11): nothing that can
/// change, no writable end and no recursive type. An alias of another type is left out of
/// the WIT; the places that use it follow the rules of § 7.11.
fn in_wit(types: &Types, ty: Type) -> bool {
    types.first_mutable(ty).is_none()
        && crate::sema::writable_end(types, ty).is_none()
        && !types
            .reachable_decls(ty)
            .into_iter()
            .any(|d| types.is_recursive(d))
}

/// WIT text of the parameters of function `f`, from the `skip`-th: `%a: u32, %b: string`.
fn wit_params(
    types: &Types,
    program: &Program,
    f: usize,
    skip: usize,
    names: &HashMap<Named, String>,
) -> String {
    program.funcs[f]
        .params
        .iter()
        .enumerate()
        .skip(skip)
        .map(|(k, (name, ty))| {
            let ty = program.spelled(Spelling::Param(f, k), *ty);
            format!("%{}: {}", kebab(name), wit_type(types, ty, names))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// WIT text of the return type of function `f`: ` -> u32`, or nothing.
fn wit_ret(types: &Types, program: &Program, f: usize, names: &HashMap<Named, String>) -> String {
    program.funcs[f]
        .ret
        .map(|t| {
            let t = program.spelled(Spelling::Ret(f), t);
            format!(" -> {}", wit_type(types, t, names))
        })
        .unwrap_or_default()
}

/// WIT text of a type; `names` gives the name of each declared type and alias in the
/// interface.
fn wit_type(types: &Types, ty: Type, names: &HashMap<Named, String>) -> String {
    match types.kind(ty) {
        TypeKind::Prim(p) => p.name().to_string(),
        TypeKind::Char => "char".into(),
        TypeKind::String => "string".into(),
        // `mut` is erased (§ 13.1).
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
        TypeKind::Borrow(inner) => format!("borrow<{}>", wit_type(types, *inner, names)),
        // Writable ends never appear in the WIT (E0222).
        TypeKind::End(kind, elem) => format!(
            "{}<{}>",
            kind.reader().name(),
            wit_type(types, *elem, names)
        ),
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
        TypeKind::Decl(d) => format!("%{}", names[&Named::Decl(*d)]),
        TypeKind::Alias(a) => format!("%{}", names[&Named::Alias(*a)]),
    }
}
