//! Semantic analysis: name resolution, typing and control flow (§ 6 to 9, order of § 11.2).
//!
//! Turns the syntax trees of all the files of a package into a [`hir::Program`].

use std::collections::HashMap;

use crate::ast::{self, ExprKind, Ident, InterfaceRef, Item, UnaryOp};
use crate::deps::Dependencies;
use crate::diag::{Code, Diagnostic, Result, err};
use crate::hir::{self, FuncId, InterfaceId};
use crate::source::{FileId, Span};
use crate::types::{
    AliasDef, CaseDef, DeclId, DeclKind, EndKind, FieldDef, Mutable, ResourceDef, Type, TypeDecl,
    TypeKind, Types,
};
use crate::wit::kebab;

mod body;
mod foreign;

pub(crate) use foreign::wasl_name;

use body::{FnCx, always_returns};

/// Maximum number of flags of a flags type (§ 7.7).
const MAX_FLAGS: usize = 32;

/// Top-level declaration.
#[derive(Clone, Copy)]
enum TopLevel {
    World,
    Interface(InterfaceId),
    Global,
}

impl TopLevel {
    fn describe(self) -> &'static str {
        match self {
            TopLevel::World => "a world",
            TopLevel::Interface(_) => "an interface",
            TopLevel::Global => "a global",
        }
    }
}

/// Analyzes the package. Items must be given in the deterministic file order; `deps`
/// holds the WIT of the packages it depends on (§ 3.5).
///
/// An error in the declarations (steps 2 to 8 of § 11.2) stops the analysis and is
/// returned. The errors of function bodies and of calls to foreign interfaces (steps 9
/// and 10) do not: each function stops at its first error, and the analysis goes on with
/// the next one. They are pushed to `errors`, and the program is only valid when
/// `errors` is empty.
pub fn analyze(
    items: &[Item],
    deps: &Dependencies,
    errors: &mut Vec<Diagnostic>,
) -> Result<hir::Program> {
    let mut a = Analyzer::new(items, deps)?;
    a.foreign_interfaces()?;
    a.items()?;
    a.foreign_items();
    a.signatures()?;
    a.worlds()?;
    a.implementations()?;
    a.handles()?;
    a.boundaries()?;
    let global_ids = a.globals()?;
    let facts = a.bodies(&global_ids, errors);
    a.unimported_calls(&facts, errors);
    a.uncarried_ends(&facts, errors);
    a.entry_calls(&facts, errors);
    a.program.types = a.types;
    Ok(a.program)
}

/// The state of the analysis, which each step of § 11.2 completes in turn.
struct Analyzer<'a> {
    deps: &'a Dependencies,
    worlds: Vec<&'a ast::World>,
    /// The interfaces of the package, then the implementations of interfaces of other
    /// packages in worlds (§ 2.4.2), which have no top-level name.
    interfaces: Vec<&'a ast::Interface>,
    globals: Vec<&'a ast::Global>,
    names: HashMap<String, TopLevel>,
    /// The `use` items of the files (§ 6.6).
    uses: Vec<&'a ast::UseDecl>,
    /// The names that `use` gives to interfaces of other packages, by file (§ 6.6).
    used: HashMap<(FileId, String), InterfaceId>,
    /// The interfaces of other packages that the worlds and the files name: their last
    /// name and their full name, for the hints of E0300 and E0305.
    foreign_full: Vec<(String, String)>,
    /// The implementation of item `k` of world `w`, by `(w, k)`.
    impls: HashMap<(usize, usize), InterfaceId>,
    converter: foreign::Converter<'a>,
    /// The interfaces of other packages that the worlds name, with the first place.
    named_foreign: Vec<(wit_parser::InterfaceId, Span)>,
    /// The name of each interface of another package in code.
    foreign_names: Vec<(String, InterfaceId)>,
    program: hir::Program,
    types: Types,
    /// The functions of the package, by id.
    funcs: Vec<&'a ast::Func>,
    /// The name of each declared type of the package, by id.
    decls: Vec<&'a Ident>,
    /// The type aliases of the package, by id (§ 7.14).
    aliases: Vec<&'a ast::AliasDecl>,
    /// The kind of each function of the package, by id.
    func_kinds: Vec<hir::FuncKind>,
    /// The functions of the interfaces of other packages, whose ids come after those of
    /// the package, until the signatures of the package are added (step 4).
    foreign_funcs: Vec<hir::FuncDef>,
    scopes: Vec<InterfaceScope>,
    /// Each implementation of an interface of another package: the implementation, the
    /// interface, and where the world exports it.
    implementations: Vec<(InterfaceId, InterfaceId, Span)>,
}

/// What the bodies of the functions use, by function, for the checks of each world
/// (steps 9 to 11).
#[derive(Default)]
struct BodyFacts {
    /// Calls to functions of interfaces of other packages.
    foreign_calls: Vec<Vec<(FuncId, Span)>>,
    /// Stream and future types.
    end_uses: Vec<Vec<(Type, Span)>>,
    /// Calls to functions that contain `task.return`.
    entry_calls: Vec<Vec<(FuncId, Span)>>,
}

impl<'a> Analyzer<'a> {
    /// 1. Top-level names (§ 6.1).
    fn new(items: &'a [Item], deps: &'a Dependencies) -> Result<Analyzer<'a>> {
        let mut worlds = Vec::new();
        let mut interfaces = Vec::new();
        let mut globals = Vec::new();
        let mut names: HashMap<String, TopLevel> = HashMap::new();

        let mut uses = Vec::new();
        for item in items {
            if let Item::Use(u) = item {
                uses.push(u);
                continue;
            }
            let name = item.name();
            if let Some(prev) = names.get(name.name.as_str()) {
                return err(
                    Code::DuplicateItem,
                    name.span,
                    format!("`{}` is already declared as {}", name.name, prev.describe()),
                );
            }
            let entry = match item {
                Item::World(w) => {
                    worlds.push(w);
                    TopLevel::World
                }
                Item::Interface(i) => {
                    interfaces.push(i);
                    TopLevel::Interface(interfaces.len() - 1)
                }
                Item::Global(g) => {
                    globals.push(g.as_ref());
                    TopLevel::Global
                }
                Item::Use(_) => unreachable!("a `use` has no top-level name"),
            };
            names.insert(name.name.clone(), entry);
        }
        // The implementations of interfaces of other packages in worlds (§ 2.4.2) are
        // interfaces of the package without a top-level name.
        let mut impls: HashMap<(usize, usize), InterfaceId> = HashMap::new();
        for (w, world) in worlds.iter().enumerate() {
            for (k, item) in world.items.iter().enumerate() {
                if let Some(body) = &item.body {
                    impls.insert((w, k), interfaces.len());
                    interfaces.push(body);
                }
            }
        }
        Ok(Analyzer {
            deps,
            converter: foreign::Converter::new(deps.resolve(), interfaces.len()),
            worlds,
            interfaces,
            globals,
            names,
            uses,
            used: HashMap::new(),
            foreign_full: Vec::new(),
            impls,
            named_foreign: Vec::new(),
            foreign_names: Vec::new(),
            program: hir::Program::default(),
            types: Types::default(),
            funcs: Vec::new(),
            decls: Vec::new(),
            aliases: Vec::new(),
            func_kinds: Vec::new(),
            foreign_funcs: Vec::new(),
            scopes: Vec::new(),
            implementations: Vec::new(),
        })
    }

    /// 2. Interfaces of other packages named by the `use` items of the files and by the
    ///    worlds (§ 2.5, § 3.5, E0215), then the names that `use` gives them, file by
    ///    file (§ 6.6, E0217).
    fn foreign_interfaces(&mut self) -> Result<()> {
        let mut used = Vec::new();
        for u in &self.uses {
            let id = foreign_interface(self.deps, &u.iface)?;
            used.push(id);
            if !self.named_foreign.iter().any(|(i, _)| *i == id) {
                self.named_foreign
                    .push((id, u.iface.namespace.span.to(u.iface.name.span)));
            }
        }
        for world in &self.worlds {
            for item in &world.items {
                let InterfaceRef::Foreign(f) = &item.iface else {
                    continue;
                };
                let id = foreign_interface(self.deps, f)?;
                if !self.named_foreign.iter().any(|(i, _)| *i == id) {
                    self.named_foreign.push((id, item.iface.span()));
                }
            }
        }
        for &(id, _) in &self.named_foreign {
            let hir_id = self.converter.iface(id);
            let short = self.converter.short_name(hir_id);
            let full = self.converter.ifaces[hir_id - self.interfaces.len()].display();
            self.foreign_full.push((short.clone(), full));
            self.foreign_names.push((short, hir_id));
        }
        for (u, id) in self.uses.iter().zip(used) {
            let hir_id = self.converter.iface(id);
            let full = u.iface.display();
            let name = u.name();
            let key = (name.span.file, name.name.clone());
            let clash = |what: String| {
                let named = match &u.alias {
                    Some(_) => format!("`{full} as {}`", name.name),
                    None => format!("`{full}`"),
                };
                err(
                    Code::InterfaceNameClash,
                    name.span,
                    format!(
                        "{named} names the interface `{}` in this file, like {what}: code could not tell them apart; give it another name with `as`",
                        name.name
                    ),
                )
            };
            if let Some(prev) = self.names.get(name.name.as_str()) {
                return clash(format!("{} of the package", prev.describe()));
            }
            if self
                .used
                .iter()
                .any(|((file, _), &i)| *file == name.span.file && i == hir_id)
            {
                return err(
                    Code::InterfaceNameClash,
                    u.iface.namespace.span.to(name.span),
                    format!("`{full}` is already used in this file: one `use` is enough"),
                );
            }
            if let Some(&other) = self.used.get(&key) {
                let other = self.converter.ifaces[other - self.interfaces.len()].display();
                return clash(format!("`{other}`"));
            }
            self.used.insert(key, hir_id);
        }
        Ok(())
    }

    /// 3. Items of the interfaces of the package (§ 6.3, § 7.7).
    fn items(&mut self) -> Result<()> {
        self.types.interface_names = self
            .interfaces
            .iter()
            .map(|i| i.name.name.clone())
            .collect();
        for (iface_id, iface) in self.interfaces.iter().enumerate() {
            let docs = &mut self.program.docs;
            let mut document = |target: hir::Doc, doc: &Option<String>| {
                if let Some(doc) = doc {
                    docs.insert(target, doc.clone());
                }
            };
            document(hir::Doc::Interface(iface_id), &iface.doc);
            let mut scope = InterfaceScope::default();
            let mut func_ids = Vec::new();
            let mut decl_ids = Vec::new();
            let mut alias_ids = Vec::new();
            for item in &iface.items {
                let name = match item {
                    ast::InterfaceItem::Func(f) => &f.name,
                    ast::InterfaceItem::Type(t) => &t.name,
                    ast::InterfaceItem::Resource(r) => &r.name,
                    ast::InterfaceItem::Alias(a) => &a.name,
                };
                let previous = if scope.funcs.contains_key(name.name.as_str()) {
                    Some("a function")
                } else if scope.types.contains_key(name.name.as_str())
                    || scope.alias_ids.contains_key(name.name.as_str())
                {
                    Some("a type")
                } else {
                    None
                };
                match (item, previous) {
                    (ast::InterfaceItem::Func(_), Some("a function")) => {
                        return err(
                            Code::DuplicateFunc,
                            name.span,
                            format!(
                                "function `{}` is already declared in interface `{}`",
                                name.name, iface.name.name
                            ),
                        );
                    }
                    (_, Some(previous)) => {
                        return err(
                            Code::DuplicateName,
                            name.span,
                            format!(
                                "`{}` is already declared as {previous} in interface `{}`: self.types and functions of an interface share one namespace",
                                name.name, iface.name.name
                            ),
                        );
                    }
                    (_, None) => {}
                }
                match item {
                    ast::InterfaceItem::Func(func) => {
                        let id = self.funcs.len();
                        document(hir::Doc::Func(id), &func.doc);
                        self.funcs.push(func);
                        self.func_kinds.push(hir::FuncKind::Free);
                        scope.funcs.insert(func.name.name.clone(), id);
                        func_ids.push(id);
                    }
                    ast::InterfaceItem::Resource(res) => {
                        if let Some(top) = self.names.get(res.name.name.as_str()) {
                            return err(
                                Code::TypeNamedLikeTopLevel,
                                res.name.span,
                                format!(
                                    "resource `{}` has the name of {} of the package",
                                    res.name.name,
                                    top.describe()
                                ),
                            );
                        }
                        check_resource_members(res)?;
                        let id = self.decls.len();
                        document(hir::Doc::Decl(id), &res.doc);
                        self.decls.push(&res.name);
                        scope.types.insert(res.name.name.clone(), id);
                        decl_ids.push(id);
                        let mut def = ResourceDef::default();
                        for rf in &res.funcs {
                            let f = self.funcs.len();
                            document(hir::Doc::Func(f), &rf.func.doc);
                            self.funcs.push(&rf.func);
                            func_ids.push(f);
                            let name = rf.func.name.name.clone();
                            self.func_kinds.push(match rf.kind {
                                ast::ResourceFuncKind::Constructor => {
                                    def.constructor = Some(f);
                                    hir::FuncKind::Constructor(id)
                                }
                                ast::ResourceFuncKind::Method => {
                                    def.methods.push((name, f));
                                    hir::FuncKind::Method(id)
                                }
                                ast::ResourceFuncKind::Static => {
                                    def.statics.push((name, f));
                                    hir::FuncKind::Static(id)
                                }
                            });
                        }
                        self.types.decls.push(TypeDecl {
                            name: res.name.name.clone(),
                            wit: None,
                            interface: iface_id,
                            kind: DeclKind::Resource(def),
                        });
                        self.types.decl_type(id);
                    }
                    ast::InterfaceItem::Type(decl) => {
                        if let Some(top) = self.names.get(decl.name.name.as_str()) {
                            return err(
                                Code::TypeNamedLikeTopLevel,
                                decl.name.span,
                                format!(
                                    "type `{}` has the name of {} of the package",
                                    decl.name.name,
                                    top.describe()
                                ),
                            );
                        }
                        check_members(decl)?;
                        let id = self.decls.len();
                        document(hir::Doc::Decl(id), &decl.doc);
                        for (k, doc) in decl.member_docs.iter().enumerate() {
                            document(hir::Doc::Member(id, k), doc);
                        }
                        self.decls.push(&decl.name);
                        scope.types.insert(decl.name.name.clone(), id);
                        decl_ids.push(id);
                        // The members are resolved in step 4; the names are known now.
                        self.types.decls.push(TypeDecl {
                            name: decl.name.name.clone(),
                            wit: None,
                            interface: iface_id,
                            kind: match &decl.kind {
                                ast::TypeDeclKind::Enum(cases) => {
                                    DeclKind::Enum(cases.iter().map(|c| c.name.clone()).collect())
                                }
                                ast::TypeDeclKind::Flags(flags) => {
                                    DeclKind::Flags(flags.iter().map(|c| c.name.clone()).collect())
                                }
                                ast::TypeDeclKind::Record(_) => DeclKind::Record(Vec::new()),
                                ast::TypeDeclKind::Variant(_) => DeclKind::Variant(Vec::new()),
                            },
                        });
                        self.types.decl_type(id);
                    }
                    ast::InterfaceItem::Alias(alias) => {
                        if let Some(top) = self.names.get(alias.name.name.as_str()) {
                            return err(
                                Code::TypeNamedLikeTopLevel,
                                alias.name.span,
                                format!(
                                    "type `{}` has the name of {} of the package",
                                    alias.name.name,
                                    top.describe()
                                ),
                            );
                        }
                        let id = self.types.aliases.len();
                        document(hir::Doc::Alias(id), &alias.doc);
                        scope.alias_ids.insert(alias.name.name.clone(), id);
                        alias_ids.push(id);
                        self.aliases.push(alias);
                        // The type is resolved in step 4, in the order of the aliases.
                        self.types.aliases.push(AliasDef {
                            name: alias.name.name.clone(),
                            interface: iface_id,
                            ty: Type::STRING,
                            spelled: Type::STRING,
                        });
                    }
                }
            }
            self.scopes.push(scope);
            self.program.interfaces.push(hir::InterfaceDef {
                name: iface.name.name.clone(),
                funcs: func_ids,
                types: decl_ids,
                aliases: alias_ids,
                foreign: None,
                implements: None,
            });
        }
        Ok(())
    }

    /// The functions and types of the interfaces of other packages, after those of the
    /// package: their ids come after.
    fn foreign_items(&mut self) {
        let mut foreign_scopes: Vec<(InterfaceId, InterfaceScope)> = Vec::new();
        for &(id, _) in &self.named_foreign {
            let (scope, defs) = self.converter.scope(
                &mut self.types,
                id,
                self.funcs.len() + self.foreign_funcs.len(),
            );
            let hir_id = self.converter.iface(id);
            foreign_scopes.push((hir_id, scope));
            self.foreign_funcs.extend(defs);
        }
        for (i, f) in self.converter.ifaces.iter().enumerate() {
            let id = self.interfaces.len() + i;
            let mut scope = foreign_scopes
                .iter_mut()
                .find(|(h, _)| *h == id)
                .map(|(_, s)| std::mem::take(s))
                .unwrap_or_default();
            scope.foreign = true;
            let func_ids: Vec<FuncId> = self
                .foreign_funcs
                .iter()
                .enumerate()
                .filter(|(_, d)| d.interface == id)
                .map(|(k, _)| self.funcs.len() + k)
                .collect();
            let decl_ids: Vec<DeclId> = (0..self.types.decls.len())
                .filter(|&d| self.types.decls[d].interface == id)
                .collect();
            self.scopes.push(scope);
            self.types
                .interface_names
                .push(self.converter.short_name(id));
            self.program.interfaces.push(hir::InterfaceDef {
                name: self.converter.short_name(id),
                funcs: func_ids,
                types: decl_ids,
                aliases: Vec::new(),
                foreign: Some(f.clone()),
                implements: None,
            });
        }
    }

    /// 4a. Type aliases (§ 7.14), each after the aliases it names; an alias that names
    ///     itself is E0225.
    fn alias_types(&mut self) -> Result<()> {
        if self.aliases.is_empty() {
            return Ok(());
        }
        let deps: Vec<Vec<usize>> = self
            .aliases
            .iter()
            .enumerate()
            .map(|(k, alias)| {
                let mut names = Vec::new();
                type_names(&alias.ty, &mut names);
                let resolver = Resolver {
                    names: &self.names,
                    used: &self.used,
                    foreign: &self.foreign_full,
                    scopes: &self.scopes,
                    spelled: true,
                };
                names
                    .into_iter()
                    .filter_map(|n| resolver.alias_id(n, Some(self.types.aliases[k].interface)))
                    .collect()
            })
            .collect();
        // Depth-first search; `state`: 0 not visited, 1 on the current path, 2 done.
        fn visit(
            k: usize,
            deps: &[Vec<usize>],
            state: &mut [u8],
            path: &mut Vec<usize>,
            order: &mut Vec<usize>,
        ) -> Option<Vec<usize>> {
            state[k] = 1;
            path.push(k);
            for &d in &deps[k] {
                if state[d] == 1 {
                    let start = path.iter().position(|&j| j == d).unwrap();
                    return Some(path[start..].to_vec());
                }
                if state[d] == 0
                    && let Some(cycle) = visit(d, deps, state, path, order)
                {
                    return Some(cycle);
                }
            }
            path.pop();
            state[k] = 2;
            order.push(k);
            None
        }
        let mut state = vec![0u8; deps.len()];
        let mut order = Vec::new();
        for k in 0..deps.len() {
            if state[k] != 0 {
                continue;
            }
            if let Some(mut cycle) = visit(k, &deps, &mut state, &mut Vec::new(), &mut order) {
                let first = (0..cycle.len()).min_by_key(|&j| cycle[j]).unwrap();
                cycle.rotate_left(first);
                let name = &self.aliases[cycle[0]].name;
                let through = if cycle.len() == 1 {
                    String::new()
                } else {
                    let others: Vec<String> = cycle[1..]
                        .iter()
                        .map(|&j| format!("`{}`", self.types.aliases[j].name))
                        .collect();
                    format!(" through {}", others.join(", "))
                };
                return err(
                    Code::AliasCycle,
                    name.span,
                    format!(
                        "type alias `{}` names itself{through}: an alias is another name of a type, not a new type; for a recursive type, declare a `variant` or a `record`",
                        name.name
                    ),
                );
            }
        }
        for k in order {
            let iface = self.types.aliases[k].interface;
            let written = &self.aliases[k].ty;
            let resolver = Resolver {
                names: &self.names,
                used: &self.used,
                foreign: &self.foreign_full,
                scopes: &self.scopes,
                spelled: false,
            };
            let ty = resolver.resolve(&mut self.types, written, Some(iface))?;
            // Like a parameter, an alias can be `borrow<T>`, which it cannot contain (E0219).
            match self.types.kind(ty) {
                TypeKind::Borrow(inner) => no_borrow(&self.types, *inner, written.span)?,
                _ => no_borrow(&self.types, ty, written.span)?,
            }
            let spelled = Resolver {
                spelled: true,
                ..resolver
            }
            .resolve(&mut self.types, written, Some(iface))?;
            let name = self.aliases[k].name.name.clone();
            self.types.aliases[k].ty = ty;
            self.types.aliases[k].spelled = spelled;
            // An alias of a declared type also names it in a literal and a `borrow`.
            if let TypeKind::Decl(d) = self.types.kind(ty) {
                self.scopes[iface].types.insert(name.clone(), *d);
            }
            self.scopes[iface].aliases.insert(name, ty);
        }
        Ok(())
    }

    /// 4. Types: fields, payloads and signatures, interface by interface (§ 11.2).
    fn signatures(&mut self) -> Result<()> {
        self.alias_types()?;
        let resolver = Resolver {
            names: &self.names,
            used: &self.used,
            foreign: &self.foreign_full,
            scopes: &self.scopes,
            spelled: false,
        };
        // The types of the WIT keep the aliases they name (§ 7.14).
        let speller = Resolver {
            spelled: true,
            ..resolver
        };
        let speller = (!self.types.aliases.is_empty()).then_some(&speller);
        let spellings = &mut self.program.spellings;
        for (iface_id, iface) in self.interfaces.iter().enumerate() {
            for item in &iface.items {
                match item {
                    ast::InterfaceItem::Type(decl) => {
                        let id = self.scopes[iface_id].types[decl.name.name.as_str()];
                        match &decl.kind {
                            ast::TypeDeclKind::Record(fields) => {
                                let mut defs = Vec::new();
                                for (k, f) in fields.iter().enumerate() {
                                    let ty =
                                        resolver.resolve(&mut self.types, &f.ty, Some(iface_id))?;
                                    no_borrow(&self.types, ty, f.ty.span)?;
                                    spell(
                                        speller,
                                        &mut self.types,
                                        spellings,
                                        hir::Spelling::Member(id, k),
                                        (&f.ty, iface_id),
                                        ty,
                                    )?;
                                    defs.push(FieldDef {
                                        name: f.name.name.clone(),
                                        mutable: f.mutable,
                                        ty,
                                    });
                                }
                                self.types.decls[id].kind = DeclKind::Record(defs);
                            }
                            ast::TypeDeclKind::Variant(cases) => {
                                let mut defs = Vec::new();
                                for (k, c) in cases.iter().enumerate() {
                                    let payload = match &c.payload {
                                        Some(ty) => {
                                            let t = resolver.resolve(
                                                &mut self.types,
                                                ty,
                                                Some(iface_id),
                                            )?;
                                            no_borrow(&self.types, t, ty.span)?;
                                            spell(
                                                speller,
                                                &mut self.types,
                                                spellings,
                                                hir::Spelling::Member(id, k),
                                                (ty, iface_id),
                                                t,
                                            )?;
                                            Some(t)
                                        }
                                        None => None,
                                    };
                                    defs.push(CaseDef {
                                        name: c.name.name.clone(),
                                        payload,
                                    });
                                }
                                self.types.decls[id].kind = DeclKind::Variant(defs);
                            }
                            ast::TypeDeclKind::Enum(_) | ast::TypeDeclKind::Flags(_) => {}
                        }
                    }
                    ast::InterfaceItem::Func(func) => {
                        let def = signature(
                            (&resolver, speller),
                            &mut self.types,
                            spellings,
                            func,
                            (hir::FuncKind::Free, self.program.funcs.len()),
                            iface_id,
                        )?;
                        self.program.funcs.push(def);
                    }
                    ast::InterfaceItem::Alias(_) => {}
                    ast::InterfaceItem::Resource(res) => {
                        let id = self.scopes[iface_id].types[res.name.name.as_str()];
                        let mut fields = Vec::new();
                        for f in &res.fields {
                            let ty = resolver.resolve(&mut self.types, &f.ty, Some(iface_id))?;
                            no_borrow(&self.types, ty, f.ty.span)?;
                            fields.push(FieldDef {
                                name: f.name.name.clone(),
                                mutable: f.mutable,
                                ty,
                            });
                        }
                        if let DeclKind::Resource(def) = &mut self.types.decls[id].kind {
                            def.fields = fields;
                        }
                        for rf in &res.funcs {
                            let id = self.program.funcs.len();
                            let def = signature(
                                (&resolver, speller),
                                &mut self.types,
                                spellings,
                                &rf.func,
                                (self.func_kinds[id], id),
                                iface_id,
                            )?;
                            self.program.funcs.push(def);
                        }
                    }
                }
            }
        }
        // Functions were pushed in interface order, which is the order of `funcs`; those of
        // other packages come after them.
        let foreign_funcs = std::mem::take(&mut self.foreign_funcs);
        self.program.funcs.extend(foreign_funcs);
        let inhabited = inhabited_decls(&self.types);
        for (id, name) in self.decls.iter().enumerate() {
            if !inhabited[id] {
                return err(
                    Code::EmptyType,
                    name.span,
                    format!(
                        "type `{}` has no value: {}; add a case without payload, or go through an `option` or a `list`",
                        name.name,
                        why_empty(&self.types, &inhabited, id)
                    ),
                );
            }
        }
        Ok(())
    }

    /// 5. Worlds (§ 6.2).
    fn worlds(&mut self) -> Result<()> {
        for (w, world) in self.worlds.iter().enumerate() {
            if let Some(doc) = &world.doc {
                self.program.docs.insert(hir::Doc::World(w), doc.clone());
            }
            let mut def = hir::WorldDef {
                name: world.name.name.clone(),
                exports: Vec::new(),
                imports: Vec::new(),
                items: Vec::new(),
            };
            for (k, item) in world.items.iter().enumerate() {
                let span = item.iface.span();
                let (id, written) = match &item.iface {
                    InterfaceRef::Local(name) => match top(&self.names, &self.used, name) {
                        Some(TopLevel::Interface(id)) if id < self.interfaces.len() => {
                            (id, name.name.clone())
                        }
                        Some(TopLevel::Interface(id)) => {
                            let full = self.program.interfaces[id]
                                .foreign
                                .as_ref()
                                .unwrap()
                                .display();
                            return err(
                                Code::UnknownExportedInterface,
                                span,
                                format!(
                                    "the package has no interface `{}`: `{}` is the interface `{full}` of another package, write `{full}`",
                                    name.name, name.name
                                ),
                            );
                        }
                        Some(other) => {
                            return err(
                                Code::UnknownExportedInterface,
                                span,
                                format!(
                                    "`{}` is {}, not an interface",
                                    name.name,
                                    other.describe()
                                ),
                            );
                        }
                        None => {
                            return err(
                                Code::UnknownExportedInterface,
                                span,
                                format!("unknown interface `{}`", name.name),
                            );
                        }
                    },
                    InterfaceRef::Foreign(f) => {
                        let id = self
                            .foreign_names
                            .iter()
                            .find(|(_, i)| {
                                self.program.interfaces[*i]
                                    .foreign
                                    .as_ref()
                                    .unwrap()
                                    .display()
                                    == f.display()
                            })
                            .map(|(_, i)| *i)
                            .expect("foreign interface resolved");
                        match self.impls.get(&(w, k)) {
                            Some(&imp) => {
                                let world_name = &world.name.name;
                                let exported = def
                                    .exports
                                    .iter()
                                    .any(|&e| self.program.interfaces[e].implements == Some(id));
                                if exported {
                                    return err(
                                        Code::DuplicateExport,
                                        span,
                                        format!(
                                            "interface `{}` is already exported by world `{world_name}`",
                                            f.display()
                                        ),
                                    );
                                }
                                if def.imports.contains(&id) {
                                    return err(
                                        Code::DuplicateImport,
                                        span,
                                        format!(
                                            "world `{world_name}` imports interface `{}`, so it cannot export it",
                                            f.display()
                                        ),
                                    );
                                }
                                self.program.interfaces[imp].implements = Some(id);
                                self.implementations.push((imp, id, span));
                                (imp, f.display())
                            }
                            None => (id, f.display()),
                        }
                    }
                };
                let world_name = &world.name.name;
                if item.import {
                    if def.imports.contains(&id) {
                        return err(
                            Code::DuplicateImport,
                            span,
                            format!(
                                "interface `{written}` is already imported by world `{world_name}`"
                            ),
                        );
                    }
                    if def.exports.contains(&id) {
                        return err(
                            Code::DuplicateImport,
                            span,
                            format!(
                                "world `{world_name}` exports interface `{written}`, so it cannot import it"
                            ),
                        );
                    }
                    def.imports.push(id);
                } else {
                    if def.exports.contains(&id) {
                        return err(
                            Code::DuplicateExport,
                            span,
                            format!(
                                "interface `{written}` is already exported by world `{world_name}`"
                            ),
                        );
                    }
                    if def.imports.contains(&id) {
                        return err(
                            Code::DuplicateImport,
                            span,
                            format!(
                                "world `{world_name}` imports interface `{written}`, so it cannot export it"
                            ),
                        );
                    }
                    def.exports.push(id);
                }
                def.items.push((item.import, id));
            }
            self.program.worlds.push(def);
        }
        Ok(())
    }

    /// The implementations of interfaces of other packages (§ 2.4.2, § 13.12): exactly
    /// the functions of the interface, with the same signatures (E0218).
    fn implementations(&mut self) -> Result<()> {
        for &(imp, foreign, span) in &self.implementations {
            check_implementation(
                &self.types,
                &self.program,
                &self.scopes,
                &self.funcs,
                imp,
                foreign,
                span,
            )?;
            // Each function is exported with the spelling of the WIT (§ 13.2).
            for k in 0..self.program.interfaces[imp].funcs.len() {
                let f = self.program.interfaces[imp].funcs[k];
                if let Some(&wit) = self.scopes[foreign].funcs.get(&self.program.funcs[f].name) {
                    self.program.funcs[f].wit = self.program.funcs[wit].wit.clone();
                }
            }
        }
        Ok(())
    }

    /// Handle types (§ 7.12): the resources of other packages and of the interfaces of the
    /// package that a world imports. They cannot be held by records, lists, fields and
    /// globals (E0220).
    fn handles(&mut self) -> Result<()> {
        self.program.handles = self
            .types
            .decls
            .iter()
            .map(|d| {
                matches!(d.kind, DeclKind::Resource(_))
                    && (self.program.interfaces[d.interface].foreign.is_some()
                        || self
                            .program
                            .worlds
                            .iter()
                            .any(|w| w.imports.contains(&d.interface)))
            })
            .collect();
        for (iface_id, iface) in self.interfaces.iter().enumerate() {
            for item in &iface.items {
                let (fields, what): (Vec<(Type, Span)>, &str) = match item {
                    ast::InterfaceItem::Type(decl) => {
                        let id = self.scopes[iface_id].types[decl.name.name.as_str()];
                        match (&decl.kind, &self.types.decls[id].kind) {
                            (ast::TypeDeclKind::Record(fields), DeclKind::Record(defs)) => (
                                defs.iter()
                                    .zip(fields)
                                    .map(|(d, f)| (d.ty, f.ty.span))
                                    .collect(),
                                "a record",
                            ),
                            (ast::TypeDeclKind::Variant(cases), DeclKind::Variant(defs)) => {
                                for (d, c) in defs.iter().zip(cases) {
                                    if let (Some(t), Some(ty)) = (d.payload, &c.payload) {
                                        no_handle_aggregate(
                                            &self.types,
                                            &self.program.handles,
                                            t,
                                            ty.span,
                                        )?;
                                    }
                                }
                                continue;
                            }
                            _ => continue,
                        }
                    }
                    ast::InterfaceItem::Resource(res) => {
                        let id = self.scopes[iface_id].types[res.name.name.as_str()];
                        let DeclKind::Resource(def) = &self.types.decls[id].kind else {
                            unreachable!("a resource")
                        };
                        (
                            def.fields
                                .iter()
                                .zip(&res.fields)
                                .map(|(d, f)| (d.ty, f.ty.span))
                                .collect(),
                            "a resource",
                        )
                    }
                    ast::InterfaceItem::Func(_) | ast::InterfaceItem::Alias(_) => continue,
                };
                for (ty, span) in fields {
                    if let Some(h) = handle_in(&self.types, &self.program.handles, ty, None) {
                        return err(
                            Code::HandlePosition,
                            span,
                            format!(
                                "`{}` is {}, and the field of {what} cannot hold one: keep it in a local",
                                self.types.display(h),
                                handle_noun(&self.types, h)
                            ),
                        );
                    }
                    no_handle_aggregate(&self.types, &self.program.handles, ty, span)?;
                }
            }
        }
        for (id, func) in self.funcs.iter().enumerate() {
            if let (Some(ret), Some(span)) = (
                self.program.funcs[id].ret,
                func.ret.as_ref().map(|r| r.span),
            ) {
                no_handle_aggregate(&self.types, &self.program.handles, ret, span)?;
            }
        }
        Ok(())
    }

    /// 6. Exported functions, and functions of the imported interfaces of the package:
    ///    types that can cross the boundary (§ 7.11). They have no parameter limit
    ///    (§ 13.6). Interfaces that appear in the WIT cannot use each other's types in a
    ///    cycle (§ 13.1).
    fn boundaries(&self) -> Result<()> {
        for world in &self.program.worlds {
            let crossing = world
                .exports
                .iter()
                .chain(&world.imports)
                .filter(|&&i| i < self.interfaces.len());
            for &iface_id in crossing {
                for &f in &self.program.interfaces[iface_id].funcs {
                    let def = &self.program.funcs[f];
                    let signature: Vec<Type> =
                        def.params.iter().map(|(_, t)| *t).chain(def.ret).collect();
                    check_boundary_types(
                        &self.types,
                        &self.program,
                        world,
                        &signature,
                        &self.funcs[f].name,
                        if world.exports.contains(&iface_id) {
                            "exported function"
                        } else {
                            "imported function"
                        },
                    )?;
                }
                // The aliases the WIT names, like the types (§ 7.14).
                self.wit_aliases(world, iface_id)?;
                // The types of an exported interface all appear in the WIT (§ 13.1).
                for &d in &self.program.interfaces[iface_id].types {
                    let ty = self.types.decl_type_of(d);
                    check_boundary_types(
                        &self.types,
                        &self.program,
                        world,
                        &[ty],
                        self.decls[d],
                        if world.exports.contains(&iface_id) {
                            "type of an exported interface"
                        } else {
                            "type of an imported interface"
                        },
                    )?;
                }
            }
        }

        // Interfaces that appear in the WIT cannot use each other's types in a cycle (§ 13.1).
        if let Some(cycle) = crate::wit::use_cycle(&self.types, &self.program) {
            let steps: Vec<String> = cycle
                .iter()
                .map(|&(i, d)| {
                    format!(
                        "`{}` uses `{}::{}`",
                        self.program.interfaces[i].name,
                        self.program.interfaces[d.interface(&self.types)].name,
                        d.name(&self.types)
                    )
                })
                .collect();
            return err(
                Code::UseCycle,
                self.interfaces[cycle[0].0].name.span,
                format!(
                    "self.interfaces use each other's self.types in a cycle, which WIT forbids: {}; move the shared self.types to an interface of their own",
                    steps.join(", ")
                ),
            );
        }
        Ok(())
    }

    /// The aliases that the WIT of interface `iface_id` names in its signatures and the
    /// members of its types, in `world`: like a declared type, an alias of an interface
    /// with functions needs the world to export or import it (E0209).
    fn wit_aliases(&self, world: &hir::WorldDef, iface_id: InterfaceId) -> Result<()> {
        if self.types.aliases.is_empty() {
            return Ok(());
        }
        let iface = &self.program.interfaces[iface_id];
        let mut written: Vec<(Type, &Ident)> = Vec::new();
        for &f in &iface.funcs {
            let def = &self.program.funcs[f];
            for (k, (_, t)) in def.params.iter().enumerate() {
                written.push((
                    self.program.spelled(hir::Spelling::Param(f, k), *t),
                    &self.funcs[f].name,
                ));
            }
            if let Some(t) = def.ret {
                written.push((
                    self.program.spelled(hir::Spelling::Ret(f), t),
                    &self.funcs[f].name,
                ));
            }
        }
        let mut members: Vec<(&hir::Spelling, &Type)> = self
            .program
            .spellings
            .iter()
            .filter(|(place, _)| {
                matches!(place, hir::Spelling::Member(d, _) if iface.types.contains(d))
            })
            .collect();
        members.sort();
        for (place, &t) in members {
            if let hir::Spelling::Member(d, _) = place {
                written.push((t, self.decls[*d]));
            }
        }
        for (ty, at) in written {
            let mut stack = vec![ty];
            while let Some(t) = stack.pop() {
                if let TypeKind::Alias(a) = self.types.kind(t) {
                    let other = self.types.aliases[*a].interface;
                    if other != iface_id
                        && !world.exports.contains(&other)
                        && !world.imports.contains(&other)
                        && !self.program.interfaces[other].funcs.is_empty()
                    {
                        return err(
                            Code::UnexportedTypeInterface,
                            at.span,
                            format!(
                                "`{}` uses the type `{}`, but world `{}` does not export interface `{}`, which has functions: export it, or move the type to an interface without functions",
                                at.name,
                                self.types.display(t),
                                world.name,
                                self.program.interfaces[other].name
                            ),
                        );
                    }
                }
                if !matches!(self.types.kind(t), TypeKind::Decl(_)) {
                    stack.extend(self.types.children(t));
                }
            }
        }
        Ok(())
    }

    /// 7. Globals (§ 6.4, § 8.18): returns the id of each global by name.
    fn globals(&mut self) -> Result<HashMap<&'a str, hir::GlobalId>> {
        let resolver = Resolver {
            names: &self.names,
            used: &self.used,
            foreign: &self.foreign_full,
            scopes: &self.scopes,
            spelled: false,
        };
        let mut global_ids = HashMap::new();
        for global in &self.globals {
            let ty = resolver.resolve(&mut self.types, &global.ty, None)?;
            if let Some(h) = handle_in(&self.types, &self.program.handles, ty, None) {
                return err(
                    Code::HandlePosition,
                    global.ty.span,
                    format!(
                        "`{}` is {}, and a global cannot hold one",
                        self.types.display(h),
                        handle_noun(&self.types, h)
                    ),
                );
            }
            if !is_constant(&global.init) {
                return err(
                    Code::NonConstGlobal,
                    global.init.span,
                    "the initial value of a global must be a constant expression, such as a literal",
                );
            }
            let mut cx = FnCx::new(
                &self.program,
                &mut self.types,
                &resolver,
                &global_ids,
                None,
                None,
            );
            let init = cx.expr(&global.init, Some(ty))?;
            if !is_constant_hir(&init) {
                return err(
                    Code::NonConstGlobal,
                    global.init.span,
                    "the initial value of a global must be a constant expression: a function call is not one",
                );
            }
            global_ids.insert(global.name.name.as_str(), self.program.globals.len());
            self.program.globals.push(hir::GlobalDef {
                name: global.name.name.clone(),
                ty,
                mutable: global.mutable,
                init,
            });
        }
        Ok(global_ids)
    }

    /// 8. Function bodies, one error at most per function (§ 11.2).
    fn bodies(
        &mut self,
        global_ids: &HashMap<&'a str, hir::GlobalId>,
        errors: &mut Vec<Diagnostic>,
    ) -> BodyFacts {
        let mut facts = BodyFacts::default();
        let resolver = Resolver {
            names: &self.names,
            used: &self.used,
            foreign: &self.foreign_full,
            scopes: &self.scopes,
            spelled: false,
        };
        for (id, func) in self.funcs.iter().enumerate() {
            let iface = self.program.funcs[id].interface;
            let ret = self.program.funcs[id].ret;
            let checked = (|| {
                let mut cx = FnCx::new(
                    &self.program,
                    &mut self.types,
                    &resolver,
                    &global_ids,
                    Some(iface),
                    ret,
                );
                cx.in_async = self.program.funcs[id].is_async;
                if let (Some(ret), Some(span)) = (ret, func.ret.as_ref().map(|r| r.span)) {
                    cx.note_ends(ret, span);
                }
                cx.scopes.push(Vec::new());
                let mut params = self.program.funcs[id].params.iter();
                if let hir::FuncKind::Method(_) = self.program.funcs[id].kind {
                    let (_, ty) = params.next().expect("self");
                    let this = Ident {
                        name: "self".into(),
                        span: func.name.span,
                    };
                    cx.declare(&this, *ty)?;
                }
                for (param, (_, ty)) in func.params.iter().zip(params) {
                    cx.declare(&param.name, *ty)?;
                }
                let body = cx.block(&func.body)?;
                if ret.is_some() && !always_returns(&body) {
                    return err(
                        Code::MissingReturn,
                        func.name.span,
                        format!(
                            "{} `{}` does not end with a `return` on every path",
                            if func.name.name == "constructor" {
                                "the constructor of"
                            } else {
                                "function"
                            },
                            func.name.name
                        ),
                    );
                }
                Ok((
                    cx.locals,
                    body,
                    cx.foreign_calls,
                    cx.end_uses,
                    cx.entry_calls,
                ))
            })();
            match checked {
                Ok((locals, body, calls, ends, entries)) => {
                    facts.foreign_calls.push(calls);
                    facts.end_uses.push(ends);
                    facts.entry_calls.push(entries);
                    let def = &mut self.program.funcs[id];
                    def.locals = locals;
                    def.body = body;
                }
                Err(d) => {
                    errors.push(d);
                    facts.foreign_calls.push(Vec::new());
                    facts.end_uses.push(Vec::new());
                    facts.entry_calls.push(Vec::new());
                }
            }
        }
        facts
    }

    /// 9. Calls to interfaces of other packages, world by world (E0221).
    fn unimported_calls(&self, facts: &BodyFacts, errors: &mut Vec<Diagnostic>) {
        let mut reported: Vec<Span> = Vec::new();
        for (w, world) in self.program.worlds.iter().enumerate() {
            let (used, _) = self.program.reach(w);
            for (f, calls) in facts.foreign_calls.iter().enumerate() {
                if !used[f] {
                    continue;
                }
                for &(callee, span) in calls {
                    let iface = self.program.funcs[callee].interface;
                    if world.imports.contains(&iface) {
                        continue;
                    }
                    let caller = &self.program.funcs[f];
                    let full = self.program.interfaces[iface]
                        .foreign
                        .as_ref()
                        .unwrap()
                        .display();
                    // One error per call, even when several worlds reach it.
                    if reported.contains(&span) {
                        continue;
                    }
                    reported.push(span);
                    errors.push(Diagnostic::new(
                        Code::UnimportedCall,
                        span,
                        format!(
                            "`{}::{}` calls `{}::{}`, of the interface `{full}`, but world `{}` reaches `{}::{}` and does not import `{full}`: add `import {full};` to world `{}`",
                            self.program.interfaces[caller.interface].name,
                            caller.name,
                            self.program.interfaces[iface].name,
                            self.program.funcs[callee].name,
                            world.name,
                            self.program.interfaces[caller.interface].name,
                            caller.name,
                            world.name,
                        ),
                    ));
                }
            }
        }
    }

    /// 10. Streams and futures, world by world: each type that the code uses appears in a
    ///     function of the world (E0223, § 7.13).
    fn uncarried_ends(&mut self, facts: &BodyFacts, errors: &mut Vec<Diagnostic>) {
        let mut reported: Vec<Span> = Vec::new();
        for (w, world) in self.program.worlds.iter().enumerate() {
            let (used, _) = self.program.reach(w);
            let carried = carried_ends(&mut self.types, &self.program, world);
            for (f, uses) in facts.end_uses.iter().enumerate() {
                if !used[f] {
                    continue;
                }
                for &(ty, span) in uses {
                    if carried.contains(&ty) || reported.contains(&span) {
                        continue;
                    }
                    reported.push(span);
                    let func = &self.program.funcs[f];
                    errors.push(Diagnostic::new(
                        Code::UncarriedEnd,
                        span,
                        format!(
                            "`{}::{}` uses `{}`, but no function of world `{}` takes or returns a `{}`: a stream or a future connects two components, and the Component Model only creates the self.types that the WIT of the world self.names",
                            self.program.interfaces[func.interface].name,
                            func.name,
                            self.types.display(ty),
                            world.name,
                            self.types.display(ty),
                        ),
                    ));
                }
            }
        }
    }

    /// 11. Calls to entry points, world by world: a function that contains `task.return`
    ///     is only called by the host and other components (E0419, § 8.21).
    fn entry_calls(&self, facts: &BodyFacts, errors: &mut Vec<Diagnostic>) {
        let mut reported: Vec<Span> = Vec::new();
        for (w, world) in self.program.worlds.iter().enumerate() {
            let (used, _) = self.program.reach(w);
            for (f, calls) in facts.entry_calls.iter().enumerate() {
                if !used[f] {
                    continue;
                }
                for &(callee, span) in calls {
                    let iface = self.program.funcs[callee].interface;
                    if world.imports.contains(&iface) || reported.contains(&span) {
                        continue;
                    }
                    reported.push(span);
                    let caller = &self.program.funcs[f];
                    errors.push(Diagnostic::new(
                        Code::EntryCall,
                        span,
                        format!(
                            "`{}::{}` calls `{}::{}`, which contains `task.return`, in the component of world `{}`: a function that delivers its result before its end is an entry point, called only by the host and other components; move the shared code into a function without `task.return`",
                            self.program.interfaces[caller.interface].name,
                            caller.name,
                            self.program.interfaces[iface].name,
                            self.program.funcs[callee].name,
                            world.name,
                        ),
                    ));
                }
            }
        }
    }
}

/// The stream and future types that the functions of the world carry: those of its
/// imported and exported interfaces (E0223).
fn carried_ends(types: &mut Types, program: &hir::Program, world: &hir::WorldDef) -> Vec<Type> {
    let mut carried = Vec::new();
    for &iface in world.imports.iter().chain(&world.exports) {
        for &f in &program.interfaces[iface].funcs {
            let func = &program.funcs[f];
            for ty in func.params.iter().map(|(_, t)| *t).chain(func.ret) {
                for (kind, elem) in ends_in(types, ty) {
                    let readable = types.intern(TypeKind::End(kind.reader(), elem));
                    if !carried.contains(&readable) {
                        carried.push(readable);
                    }
                }
            }
        }
    }
    carried
}

/// Constant expression, the only initial value of a global (§ 8.18). Calls are
/// accepted here when their arguments are constant; [`is_constant_hir`] then keeps only
/// constructors.
fn is_constant(e: &ast::Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Char(_)
        | ExprKind::Path(_)
        | ExprKind::Flags { .. } => true,
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => matches!(expr.kind, ExprKind::Int(_) | ExprKind::Float(_)),
        ExprKind::Call(call) => call.args.iter().all(is_constant),
        ExprKind::Record { fields, .. } => fields.iter().all(|(_, v)| is_constant(v)),
        ExprKind::List(elems) | ExprKind::Tuple(elems) => elems.iter().all(is_constant),
        _ => false,
    }
}

fn is_constant_hir(e: &hir::Expr) -> bool {
    use hir::ExprKind as K;
    match &e.kind {
        K::Const(_) | K::Str(_) | K::None => true,
        K::Some(inner) => is_constant_hir(inner),
        K::Case { payload, .. } => payload.as_deref().is_none_or(is_constant_hir),
        K::Record(values) | K::List(values) | K::Tuple(values) => {
            values.iter().all(is_constant_hir)
        }
        _ => false,
    }
}

/// Functions and types of an interface, by name.
#[derive(Default)]
pub(super) struct InterfaceScope {
    funcs: HashMap<String, FuncId>,
    types: HashMap<String, DeclId>,
    /// Types of another package that are aliases of a type that is not declared:
    /// `type instant = u64` (§ 13.2).
    aliases: HashMap<String, Type>,
    /// Type aliases of the package (§ 7.14), by name: their index in [`Types::aliases`].
    alias_ids: HashMap<String, usize>,
    /// Items of another package that code cannot use, with the reason (E0215).
    unavailable: HashMap<String, String>,
    /// An interface of another package.
    foreign: bool,
}

impl InterfaceScope {
    /// The error for an item of another package that code cannot use, if `name` is one.
    fn unavailable<T>(&self, iface: &str, name: &Ident) -> Option<Result<T>> {
        let reason = self.unavailable.get(name.name.as_str())?;
        Some(err(
            Code::ForeignInterface,
            name.span,
            format!(
                "`{iface}::{}` cannot be used from WaSL: {reason}",
                name.name
            ),
        ))
    }
}

/// The interface of a dependency named `namespace:package/interface` in a world (E0215).
fn foreign_interface(deps: &Dependencies, f: &ast::ForeignName) -> Result<wit_parser::InterfaceId> {
    let package = format!("{}:{}", kebab(&f.namespace.name), kebab(&f.package.name));
    let span = f.namespace.span.to(f.name.span);
    let Some(id) = deps.package(&package) else {
        return err(
            Code::ForeignInterface,
            span,
            format!(
                "`{}` is not a dependency of the package: add `\"{package}\"` to `[dependencies]` in `wasl.toml`",
                package.replace('-', "_")
            ),
        );
    };
    let pkg = &deps.resolve().packages[id];
    match pkg.interfaces.get(&kebab(&f.name.name)) {
        Some(&iface) => Ok(iface),
        None => {
            let mut known: Vec<String> = pkg
                .interfaces
                .keys()
                .filter_map(|n| wasl_name(n))
                .map(|n| format!("`{n}`"))
                .collect();
            known.sort();
            err(
                Code::ForeignInterface,
                f.name.span,
                format!(
                    "package `{}` has no interface `{}`{}",
                    f.namespace.name.clone() + ":" + &f.package.name,
                    f.name.name,
                    if known.is_empty() {
                        String::new()
                    } else {
                        format!("; its interfaces are {}", known.join(", "))
                    }
                ),
            )
        }
    }
}

/// The signature of a function: its parameters, with `self` first for a method, and its
/// return type, the resource for a constructor. With a speller, the types that name an
/// alias of the package are also kept as written, for the WIT (§ 7.14).
fn signature(
    (resolver, speller): (&Resolver, Option<&Resolver>),
    types: &mut Types,
    spellings: &mut HashMap<hir::Spelling, Type>,
    func: &ast::Func,
    (kind, id): (hir::FuncKind, FuncId),
    iface: InterfaceId,
) -> Result<hir::FuncDef> {
    let mut params = Vec::new();
    if let hir::FuncKind::Method(r) = kind {
        let ty = types.decl_type(r);
        params.push(("self".to_string(), types.intern(TypeKind::Borrow(ty))));
    }
    for p in &func.params {
        let ty = resolver.resolve(types, &p.ty, Some(iface))?;
        // `borrow<T>` is the type of a parameter, never part of one (E0219).
        if let TypeKind::Borrow(inner) = types.kind(ty) {
            no_borrow(types, *inner, p.ty.span)?;
        } else {
            no_borrow(types, ty, p.ty.span)?;
        }
        let place = hir::Spelling::Param(id, params.len());
        spell(speller, types, spellings, place, (&p.ty, iface), ty)?;
        params.push((p.name.name.clone(), ty));
    }
    let ret = match (&func.ret, kind) {
        (_, hir::FuncKind::Constructor(r)) => Some(types.decl_type(r)),
        (Some(ty), _) => {
            let t = resolver.resolve(types, ty, Some(iface))?;
            no_borrow(types, t, ty.span)?;
            spell(
                speller,
                types,
                spellings,
                hir::Spelling::Ret(id),
                (ty, iface),
                t,
            )?;
            Some(t)
        }
        (None, _) => None,
    };
    Ok(hir::FuncDef {
        name: func.name.name.clone(),
        wit: None,
        kind,
        is_async: func.is_async,
        delivers: func
            .body
            .stmts
            .iter()
            .any(|s| matches!(s.kind, ast::StmtKind::TaskReturn(_))),
        interface: iface,
        params,
        ret,
        locals: Vec::new(),
        body: Vec::new(),
    })
}

/// Keeps the type written at `place` with its aliases, when it names one (§ 7.14).
fn spell(
    speller: Option<&Resolver>,
    types: &mut Types,
    spellings: &mut HashMap<hir::Spelling, Type>,
    place: hir::Spelling,
    (written, iface): (&ast::TypeExpr, InterfaceId),
    ty: Type,
) -> Result<()> {
    if let Some(speller) = speller {
        let spelled = speller.resolve(types, written, Some(iface))?;
        if spelled != ty {
            spellings.insert(place, spelled);
        }
    }
    Ok(())
}

/// The names of declared types and aliases written in a type.
fn type_names<'t>(ty: &'t ast::TypeExpr, names: &mut Vec<&'t ast::TypeName>) {
    use ast::TypeExprKind as K;
    match &ty.kind {
        K::Prim(_) | K::Char | K::String => {}
        K::List { elem, .. } | K::Option(elem) | K::End(_, elem) => type_names(elem, names),
        K::Tuple(elems) => elems.iter().for_each(|e| type_names(e, names)),
        K::Result { ok, err } => ok.iter().chain(err).for_each(|e| type_names(e, names)),
        K::Named(name) | K::Borrow(name) => names.push(name),
    }
}

/// `borrow<T>` anywhere in `ty`, which is not the type of a parameter (E0219).
pub(super) fn no_borrow(types: &Types, ty: Type, span: Span) -> Result<()> {
    let mut stack = vec![ty];
    while let Some(t) = stack.pop() {
        if let TypeKind::Borrow(_) = types.kind(t) {
            return err(
                Code::BorrowMisuse,
                span,
                format!(
                    "`{}` is only allowed as the type of a parameter: a borrowed resource cannot be kept, stored or returned",
                    types.display(t)
                ),
            );
        }
        if types.decl(t).is_none() {
            stack.extend(types.children(t));
        }
    }
    Ok(())
}

/// Fields and functions of a resource declared twice, or two constructors (E0202).
fn check_resource_members(res: &ast::ResourceDecl) -> Result<()> {
    let mut seen: Vec<&str> = Vec::new();
    let names = res
        .fields
        .iter()
        .map(|f| &f.name)
        .chain(res.funcs.iter().map(|f| &f.func.name));
    for name in names {
        if seen.contains(&name.name.as_str()) {
            let what = if name.name == "constructor" {
                format!("resource `{}` already has a constructor", res.name.name)
            } else {
                format!(
                    "`{}` is already declared in resource `{}`: its fields and functions share one namespace",
                    name.name, res.name.name
                )
            };
            return err(Code::DuplicateFunc, name.span, what);
        }
        seen.push(&name.name);
    }
    Ok(())
}

/// Fields, cases or flags declared twice (E0207), more than 32 flags (E0212).
fn check_members(decl: &ast::TypeDecl) -> Result<()> {
    let (what, names): (&str, Vec<&Ident>) = match &decl.kind {
        ast::TypeDeclKind::Record(fields) => ("field", fields.iter().map(|f| &f.name).collect()),
        ast::TypeDeclKind::Variant(cases) => ("case", cases.iter().map(|c| &c.name).collect()),
        ast::TypeDeclKind::Enum(cases) => ("case", cases.iter().collect()),
        ast::TypeDeclKind::Flags(flags) => ("flag", flags.iter().collect()),
    };
    for (i, name) in names.iter().enumerate() {
        if names[..i].iter().any(|n| n.name == name.name) {
            return err(
                Code::DuplicateMember,
                name.span,
                format!(
                    "{what} `{}` is already declared in `{}`",
                    name.name, decl.name.name
                ),
            );
        }
    }
    if what == "flag" && names.len() > MAX_FLAGS {
        return err(
            Code::TooManyFlags,
            decl.name.span,
            format!(
                "flags type `{}` has {} flags, the maximum is {MAX_FLAGS}",
                decl.name.name,
                names.len()
            ),
        );
    }
    Ok(())
}

/// Which declared types have a value (§ 7.9): a record has one if all its fields have
/// one, a variant if at least one of its cases has one (a case without payload always
/// has one), an enum and a flags type always. Among the other types, a tuple has a value
/// if all its elements have one, a `result` if one of its cases has one, and the others
/// (scalars, `string`, `list`, `option`) always. Computed as a least fixed point.
fn inhabited_decls(types: &Types) -> Vec<bool> {
    let mut inhabited = vec![false; types.decls.len()];
    loop {
        let mut changed = false;
        for id in 0..types.decls.len() {
            if !inhabited[id] && decl_has_value(types, &inhabited, id) {
                inhabited[id] = true;
                changed = true;
            }
        }
        if !changed {
            return inhabited;
        }
    }
}

fn decl_has_value(types: &Types, inhabited: &[bool], id: DeclId) -> bool {
    match &types.decls[id].kind {
        DeclKind::Record(fields) => fields.iter().all(|f| has_value(types, inhabited, f.ty)),
        DeclKind::Resource(r) => r.fields.iter().all(|f| has_value(types, inhabited, f.ty)),
        DeclKind::Variant(cases) => cases
            .iter()
            .any(|c| c.payload.is_none_or(|t| has_value(types, inhabited, t))),
        DeclKind::Enum(_) | DeclKind::Flags(_) => true,
    }
}

fn has_value(types: &Types, inhabited: &[bool], ty: Type) -> bool {
    match types.kind(ty) {
        TypeKind::Prim(_)
        | TypeKind::Char
        | TypeKind::String
        | TypeKind::List { .. }
        | TypeKind::Option(_)
        | TypeKind::Borrow(_)
        | TypeKind::End(..) => true,
        TypeKind::Tuple(elems) => elems.iter().all(|&t| has_value(types, inhabited, t)),
        TypeKind::Result { ok, err } => {
            ok.is_none_or(|t| has_value(types, inhabited, t))
                || err.is_none_or(|t| has_value(types, inhabited, t))
        }
        TypeKind::Decl(d) => inhabited[*d],
        TypeKind::Alias(a) => has_value(types, inhabited, types.aliases[*a].ty),
    }
}

/// Why declared type `id`, which has no value, has none: its first field without a value,
/// or the cases of a variant.
fn why_empty(types: &Types, inhabited: &[bool], id: DeclId) -> String {
    let display = |t: Type| format!("`{}`", types.display(t));
    let fields = match &types.decls[id].kind {
        DeclKind::Record(fields) => Some(fields),
        DeclKind::Resource(r) => Some(&r.fields),
        _ => None,
    };
    match fields {
        Some(fields) => {
            let field = fields
                .iter()
                .find(|f| !has_value(types, inhabited, f.ty))
                .expect("a record without a value has a field without a value");
            format!(
                "its field `{}` of type {} has no value, so building one already needs one",
                field.name,
                display(field.ty)
            )
        }
        _ => "each of its cases contains a type without a value, so building one already needs one"
            .to_string(),
    }
}

/// Types used at the boundary of a component by `world` (§ 7.11): no recursive type
/// (E0208), only types of interfaces that the world exports or that have no function
/// (E0209), nothing that can change (E0214), and no writable end (E0222).
fn check_boundary_types(
    types: &Types,
    program: &hir::Program,
    world: &hir::WorldDef,
    used: &[Type],
    at: &Ident,
    what: &str,
) -> Result<()> {
    for &ty in used {
        for id in types.reachable_decls(ty) {
            let decl = &types.decls[id];
            if types.is_recursive(id) {
                return err(
                    Code::RecursiveExport,
                    at.span,
                    format!(
                        "{what} `{}` uses the recursive type `{}`: recursive types cannot cross the boundary of a component",
                        at.name,
                        types.display(types.decl_type_of(id))
                    ),
                );
            }
            let iface = &program.interfaces[decl.interface];
            let implemented = world
                .exports
                .iter()
                .any(|&e| program.interfaces[e].implements == Some(decl.interface));
            if let Some(f) = &iface.foreign
                && !world.imports.contains(&decl.interface)
                && !implemented
                && !iface.funcs.is_empty()
            {
                return err(
                    Code::UnexportedTypeInterface,
                    at.span,
                    format!(
                        "{what} `{}` uses the type `{}`, but world `{}` does not import interface `{}`, which has functions: add `import {};` to the world",
                        at.name,
                        types.display(types.decl_type_of(id)),
                        world.name,
                        f.display(),
                        f.display()
                    ),
                );
            }
            if !world.exports.contains(&decl.interface)
                && !world.imports.contains(&decl.interface)
                && !iface.funcs.is_empty()
            {
                return err(
                    Code::UnexportedTypeInterface,
                    at.span,
                    format!(
                        "{what} `{}` uses the type `{}`, but world `{}` does not export interface `{}`, which has functions: export it, or move the type to an interface without functions",
                        at.name,
                        types.display(types.decl_type_of(id)),
                        world.name,
                        iface.name
                    ),
                );
            }
        }
    }
    for &ty in used {
        if let Some(writer) = writable_end(types, ty) {
            let (kind, elem) = types.end(writer).expect("an end");
            return err(
                Code::WriterInWit,
                at.span,
                format!(
                    "{what} `{}` uses `{}`, a writable end, but the WIT only names readable ends: give the other component a `{}<{}>` and keep the writer in this one (§ 7.13)",
                    at.name,
                    types.display(writer),
                    kind.reader().name(),
                    types.display(elem)
                ),
            );
        }
    }
    for &ty in used {
        let message = match types.first_mutable(ty) {
            None => continue,
            Some(Mutable::Field(id, field)) => format!(
                "field `{field}` of record `{}` is `mut`, but the record appears in the WIT through the {what} `{}`, and the WIT has no mutability: remove `mut` and build a new record to change a value, or keep the record out of the WIT",
                types.display(types.decl_type_of(id)),
                at.name
            ),
            Some(Mutable::List(list)) => {
                let TypeKind::List { elem, .. } = types.kind(list) else {
                    unreachable!("a mutable list")
                };
                format!(
                    "`{}` appears in the WIT through the {what} `{}`, but the WIT has no mutability: use `list<{}>`, and `list.copy` for a mutable copy inside the component",
                    types.display(list),
                    at.name,
                    types.display(*elem)
                )
            }
        };
        return err(Code::MutableExport, at.span, message);
    }
    Ok(())
}

/// What `name` designates at the top level of its file: a world, an interface or a global
/// of the package, or an interface that the file uses (§ 6.1, § 6.6).
fn top(
    names: &HashMap<String, TopLevel>,
    used: &HashMap<(FileId, String), InterfaceId>,
    name: &Ident,
) -> Option<TopLevel> {
    names.get(name.name.as_str()).copied().or_else(|| {
        used.get(&(name.span.file, name.name.clone()))
            .map(|&i| TopLevel::Interface(i))
    })
}

/// Resolves the types written in the source (§ 7.8).
struct Resolver<'a> {
    names: &'a HashMap<String, TopLevel>,
    /// The names that `use` gives, by file (§ 6.6).
    used: &'a HashMap<(FileId, String), InterfaceId>,
    /// The last and full names of the interfaces of other packages, for hints.
    foreign: &'a [(String, String)],
    scopes: &'a [InterfaceScope],
    /// Keep the aliases of the package, as written, for the WIT (§ 7.14), rather than
    /// the types they name.
    spelled: bool,
}

impl Resolver<'_> {
    /// The name code gives to an interface.
    fn interface_name(&self, iface: InterfaceId) -> String {
        self.names
            .iter()
            .find(|(_, t)| matches!(t, TopLevel::Interface(i) if *i == iface))
            .map(|(n, _)| n.clone())
            .or_else(|| {
                let mut names: Vec<&String> = self
                    .used
                    .iter()
                    .filter(|&(_, &i)| i == iface)
                    .map(|((_, n), _)| n)
                    .collect();
                names.sort();
                names.first().map(|n| n.to_string())
            })
            .unwrap_or_default()
    }

    /// The hint for an unknown interface `name` that names an interface of another
    /// package: the file needs a `use` (§ 6.6).
    pub(super) fn missing_use(&self, name: &Ident) -> String {
        match self.foreign.iter().find(|(short, _)| *short == name.name) {
            Some((_, full)) => format!(": add `use {full};` at the top of the file"),
            None => String::new(),
        }
    }

    /// What `name` designates at the top level of its file (§ 6.1, § 6.6).
    pub(super) fn top(&self, name: &Ident) -> Option<TopLevel> {
        top(self.names, self.used, name)
    }

    /// The type designated by `ty`, written in interface `iface` (`None` for a global).
    fn resolve(
        &self,
        types: &mut Types,
        ty: &ast::TypeExpr,
        iface: Option<InterfaceId>,
    ) -> Result<Type> {
        use ast::TypeExprKind as K;
        let kind = match &ty.kind {
            K::Prim(p) => return Ok(Type::prim(*p)),
            K::Char => return Ok(Type::CHAR),
            K::String => return Ok(Type::STRING),
            K::List { mutable, elem } => TypeKind::List {
                mutable: *mutable,
                elem: self.resolve(types, elem, iface)?,
            },
            K::Tuple(elems) => TypeKind::Tuple(
                elems
                    .iter()
                    .map(|e| self.resolve(types, e, iface))
                    .collect::<Result<_>>()?,
            ),
            K::Option(inner) => TypeKind::Option(self.resolve(types, inner, iface)?),
            K::Result { ok, err } => TypeKind::Result {
                ok: ok
                    .as_ref()
                    .map(|t| self.resolve(types, t, iface))
                    .transpose()?,
                err: err
                    .as_ref()
                    .map(|t| self.resolve(types, t, iface))
                    .transpose()?,
            },
            K::Named(name) => {
                if self.spelled
                    && let Some(a) = self.alias_id(name, iface)
                {
                    return Ok(types.intern(TypeKind::Alias(a)));
                }
                if let Some(&t) = self.alias(name, iface) {
                    return Ok(t);
                }
                TypeKind::Decl(self.decl(name, iface)?)
            }
            K::Borrow(name) => {
                let decl = self.decl(name, iface)?;
                if !matches!(types.decls[decl].kind, DeclKind::Resource(_)) {
                    return err(
                        Code::BorrowMisuse,
                        name.span(),
                        format!(
                            "`borrow<{}>`: only a resource can be borrowed, and `{}` is a {}",
                            types.display(types.decl_type_of(decl)),
                            name.name.name,
                            types.decls[decl].kind.keyword()
                        ),
                    );
                }
                TypeKind::Borrow(types.decl_type(decl))
            }
            K::End(kind, elem) => TypeKind::End(*kind, self.resolve(types, elem, iface)?),
        };
        Ok(types.intern(kind))
    }

    /// The type of another package named `name`, when it is an alias of a type that is
    /// not declared (`type instant = u64`).
    fn alias(&self, name: &ast::TypeName, current: Option<InterfaceId>) -> Option<&Type> {
        let iface = match &name.iface {
            Some(iface) => match self.top(iface) {
                Some(TopLevel::Interface(id)) => id,
                _ => return None,
            },
            None => current?,
        };
        self.scopes[iface].aliases.get(name.name.name.as_str())
    }

    /// The alias of the package named `name` (§ 7.14), by its index in [`Types::aliases`].
    fn alias_id(&self, name: &ast::TypeName, current: Option<InterfaceId>) -> Option<usize> {
        let iface = match &name.iface {
            Some(iface) => match self.top(iface) {
                Some(TopLevel::Interface(id)) => id,
                _ => return None,
            },
            None => current?,
        };
        self.scopes
            .get(iface)?
            .alias_ids
            .get(name.name.name.as_str())
            .copied()
    }

    /// The declared type named `name`.
    fn decl(&self, name: &ast::TypeName, current: Option<InterfaceId>) -> Result<DeclId> {
        let unknown = |message: String| err(Code::UnknownType, name.span(), message);
        let iface = match &name.iface {
            Some(iface) => match self.top(iface) {
                Some(TopLevel::Interface(id)) => id,
                Some(other) => {
                    return unknown(format!(
                        "`{}` is {}, not an interface",
                        iface.name,
                        other.describe()
                    ));
                }
                None => {
                    return err(
                        Code::UnknownName,
                        iface.span,
                        format!(
                            "unknown interface `{}`{}",
                            iface.name,
                            self.missing_use(iface)
                        ),
                    );
                }
            },
            None => match current {
                Some(id) => id,
                None => {
                    let hint = self
                        .names
                        .iter()
                        .filter_map(|(n, t)| match t {
                            TopLevel::Interface(i)
                                if self.scopes[*i].types.contains_key(name.name.name.as_str()) =>
                            {
                                Some(n.as_str())
                            }
                            _ => None,
                        })
                        .min()
                        .map(|i| format!(": write `{i}::{}`", name.name.name))
                        .unwrap_or_default();
                    return unknown(format!(
                        "unknown type `{}` (outside an interface, a type is named with its interface){hint}",
                        name.name.name
                    ));
                }
            },
        };
        let scope = &self.scopes[iface];
        if let Some(&id) = scope.types.get(name.name.name.as_str()) {
            return Ok(id);
        }
        if let Some(e) = scope.unavailable(&self.interface_name(iface), &name.name) {
            return e;
        }
        if scope.aliases.contains_key(name.name.name.as_str()) {
            return unknown(format!(
                "`{}` is an alias of a type that is not a record or a flags type",
                name.name.name
            ));
        }
        if scope.funcs.contains_key(name.name.name.as_str()) {
            return unknown(format!("`{}` is a function, not a type", name.name.name));
        }
        let mut message = format!("unknown type `{}`", name.name.name);
        if name.iface.is_none()
            && let Some(other) = self
                .names
                .iter()
                .filter_map(|(n, t)| match t {
                    TopLevel::Interface(i)
                        if self.scopes[*i].types.contains_key(name.name.name.as_str()) =>
                    {
                        Some(n.as_str())
                    }
                    _ => None,
                })
                .min()
        {
            message += &format!(
                "; to use the one of another interface, write `{other}::{}`",
                name.name.name
            );
        }
        unknown(message)
    }
}

/// The ends of streams and futures in `ty`, at any depth, with their element types.
pub(super) fn ends_in(types: &Types, ty: Type) -> Vec<(EndKind, Type)> {
    let mut seen = Vec::new();
    let mut ends = Vec::new();
    let mut stack = vec![ty];
    while let Some(t) = stack.pop() {
        if seen.contains(&t) {
            continue;
        }
        seen.push(t);
        if let Some(end) = types.end(t) {
            ends.push(end);
        }
        // The fields of a declared type are reached through its declaration, which
        // never holds an end (E0220).
        if !matches!(types.kind(t), TypeKind::Decl(_)) {
            stack.extend(types.children(t));
        }
    }
    ends
}

/// The first `stream_writer<T>` or `future_writer<T>` in `ty`, at any depth (E0222).
pub(crate) fn writable_end(types: &Types, ty: Type) -> Option<Type> {
    let mut seen = Vec::new();
    let mut stack = vec![ty];
    while let Some(t) = stack.pop() {
        if seen.contains(&t) {
            continue;
        }
        seen.push(t);
        if types.end(t).is_some_and(|(kind, _)| !kind.readable()) {
            return Some(t);
        }
        let mut children = types.children(t);
        children.reverse();
        stack.extend(children);
    }
    None
}

/// What a handle type is, for messages: a resource of another component, or an end of a
/// stream or a future.
pub(super) fn handle_noun(types: &Types, h: Type) -> &'static str {
    match types.end(h) {
        Some((kind, _)) if kind.is_stream() => "an end of a stream, which is a handle",
        Some(_) => "an end of a future, which is a handle",
        None => "a handle to a resource of another component",
    }
}

/// Whether `ty` is a handle itself (§ 7.12): an `own` or `borrow` of a resource, or an end
/// of a stream or a future.
pub(super) fn is_handle_type(types: &Types, ty: Type) -> bool {
    types.resource(ty).is_some() || types.end(ty).is_some()
}

/// The handle type that `ty` is or contains (§ 7.12), if any, through elements, fields and
/// payloads; the resources of interface `except` do not count. An end of a stream or a
/// future is a handle.
pub(super) fn handle_in(
    types: &Types,
    handles: &[bool],
    ty: Type,
    except: Option<InterfaceId>,
) -> Option<Type> {
    let mut seen = Vec::new();
    let mut stack = vec![ty];
    while let Some(t) = stack.pop() {
        if seen.contains(&t) {
            continue;
        }
        seen.push(t);
        if types.end(t).is_some() {
            return Some(t);
        }
        if let Some((d, _)) = types.resource(t)
            && handles[d]
            && Some(types.decls[d].interface) != except
        {
            return Some(types.decl_type_of(d));
        }
        stack.extend(types.children(t));
    }
    None
}

/// E0220 for a list in `ty` that holds a handle: only `option`, `result`, variants and
/// tuples can carry one, from a call to `return`, `?`, `match`, `_ =` or a destructuring
/// (§ 7.12).
pub(super) fn no_handle_aggregate(
    types: &Types,
    handles: &[bool],
    ty: Type,
    span: Span,
) -> Result<()> {
    let mut stack = vec![ty];
    let mut seen = Vec::new();
    while let Some(t) = stack.pop() {
        if seen.contains(&t) {
            continue;
        }
        seen.push(t);
        if matches!(types.kind(t), TypeKind::List { .. })
            && let Some(h) = handle_in(types, handles, t, None)
        {
            return err(
                Code::HandlePosition,
                span,
                format!(
                    "`{}` holds `{}`, {}, and a list cannot hold one: only `option`, `result`, variants and tuples can carry a handle",
                    types.display(t),
                    types.display(h),
                    handle_noun(types, h),
                ),
            );
        }
        if let Some((kind, elem)) = types.end(t) {
            if kind.is_stream()
                && let Some(h) = handle_in(types, handles, elem, None)
            {
                return err(
                    Code::HandlePosition,
                    span,
                    format!(
                        "`{}` carries `{}`, {}: the elements of a stream are written and read as lists, and a list cannot hold a handle",
                        types.display(t),
                        types.display(h),
                        handle_noun(types, h),
                    ),
                );
            }
            // The value of a future can hold handles.
            continue;
        }
        if !matches!(types.kind(t), TypeKind::Decl(_)) {
            stack.extend(types.children(t));
        }
    }
    Ok(())
}

/// E0218: the implementation `imp` of the interface `foreign` of another package has
/// exactly its functions, with the same parameter names and types and the same result.
fn check_implementation(
    types: &Types,
    program: &hir::Program,
    scopes: &[InterfaceScope],
    funcs: &[&ast::Func],
    imp: InterfaceId,
    foreign: InterfaceId,
    span: Span,
) -> Result<()> {
    let wit = program.interfaces[foreign]
        .foreign
        .as_ref()
        .expect("foreign")
        .display();
    let mut unavailable: Vec<(&String, &String)> = scopes[foreign].unavailable.iter().collect();
    unavailable.sort();
    if let Some((name, reason)) = unavailable.first() {
        return err(
            Code::ForeignExport,
            span,
            format!("`{wit}` cannot be implemented in WaSL yet: `{name}`: {reason}"),
        );
    }
    if let Some(&d) = program.interfaces[foreign]
        .types
        .iter()
        .find(|&&d| matches!(types.decls[d].kind, DeclKind::Resource(_)))
    {
        return err(
            Code::ForeignExport,
            span,
            format!(
                "`{wit}` declares the resource `{}`, and implementing resources of other packages is not available yet",
                types.decls[d].name
            ),
        );
    }
    let mine = &program.interfaces[imp].funcs;
    for &f in &program.interfaces[foreign].funcs {
        let want = &program.funcs[f];
        let Some(&g) = mine.iter().find(|&&g| program.funcs[g].name == want.name) else {
            return err(
                Code::ForeignExport,
                span,
                format!(
                    "the function `{}` of `{wit}` is missing: {}",
                    want.name,
                    signature_text(types, want)
                ),
            );
        };
        let have = &program.funcs[g];
        if have.params != want.params || have.ret != want.ret || have.is_async != want.is_async {
            return err(
                Code::ForeignExport,
                funcs[g].name.span,
                format!(
                    "`{}` does not have the signature of `{wit}`: expected {}, found {}",
                    want.name,
                    signature_text(types, want),
                    signature_text(types, have)
                ),
            );
        }
    }
    for &g in mine {
        let name = &program.funcs[g].name;
        if !program.interfaces[foreign]
            .funcs
            .iter()
            .any(|&f| program.funcs[f].name == *name)
        {
            return err(
                Code::ForeignExport,
                funcs[g].name.span,
                format!(
                    "`{name}` is not a function of `{wit}`: an implementation has exactly the functions of its interface"
                ),
            );
        }
    }
    Ok(())
}

/// `func run() -> result`, as written in WaSL.
fn signature_text(types: &Types, f: &hir::FuncDef) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|(n, t)| format!("{n}: {}", types.display(*t)))
        .collect();
    let ret = f
        .ret
        .map(|t| format!(" -> {}", types.display(t)))
        .unwrap_or_default();
    let func = if f.is_async { "async func" } else { "func" };
    format!("`{func} {}({}){ret}`", f.name, params.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::Const;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::source::FileId;

    /// Analyzes `text`, and returns its first error.
    fn analyze_str(text: &str) -> Result<hir::Program> {
        let mut errors = Vec::new();
        let program = analyze(
            &parse(tokenize(FileId(0), text)?)?,
            &Dependencies::default(),
            &mut errors,
        )?;
        match errors.into_iter().next() {
            Some(d) => Err(d),
            None => Ok(program),
        }
    }

    /// Analyzes a program made of a global `g`, a function `f(n: u32, x: f64, b: bool) -> u32` and the given body.
    fn body(text: &str) -> Result<hir::Program> {
        analyze_str(&format!(
            "global g: u32 = 1;
             global m: mut u32 = 1;
             interface i {{
               func f(n: u32, x: f64, b: bool) -> u32 {{ {text} }}
               func v() {{ }}
             }}
             interface other {{ func o() -> u32 {{ return 1; }} }}"
        ))
    }

    fn ok(text: &str) {
        if let Err(e) = body(text) {
            panic!("{text}: {:?} {}", e.code, e.message);
        }
    }

    fn fails(text: &str, code: Code) {
        match body(text) {
            Ok(_) => panic!("{text}: accepted"),
            Err(e) => assert_eq!(e.code, code, "{text}: {}", e.message),
        }
    }

    #[test]
    fn valid_bodies() {
        ok("return n;");
        ok(
            "local a: u64 = 0; local i: u32; while (i < n) { local next: u64 = a + 1; a = next; i = i + 1; } return u32(a);",
        );
        ok("if (n < 1) { return 0; } else if (n > 2) { return 1; } else { return 2; }");
        ok("local s: s8 = -128; local t: s8 = -(5); return u32(s) + u32(t);");
        ok("local y: f64 = x * 2.0 + -1.5; return u32(y);");
        ok("m = m + 1; v(); other::o(); return other::o() + g;");
        ok("local c: bool = n > 1 && !b || x == 2.0; return 1 + 2 * 3;");
        ok("while (true) { if (b) { break; } else { continue; } } return n << 2 & 255;");
        ok("n = 5; return n;");
    }

    #[test]
    fn integer_functions() {
        ok("return u32.wrapping_add(n, 1) + u32.wrapping_mul(2, 3) - u32.wrapping_sub(n, n);");
        ok("local s: s8 = s8.wrapping_neg(-128); return u32.wrapping_from(s);");
        ok(
            "local o: option<u64> = u64.checked_mul(u64(n), 3); m = u32.wrapping_from(m); return n;",
        );
        ok("u8.wrapping_add(1, 2); return 0;");
        fails("return u32.wrapping_neg(n);", Code::UnknownFunc);
        fails("return u32.saturating_add(n, 1);", Code::UnknownFunc);
        ok(
            "local t: string = u32.to_string(n) + s8.to_string(-1); local o: option<u32> = u32.parse(t); return n;",
        );
        fails(
            "local t: string = u32.to_string(b); return n;",
            Code::TypeMismatch,
        );
        fails(
            "local t: string = u32.to_string(n, 10); return n;",
            Code::ArgCount,
        );
        fails(
            "local o: option<u64> = u32.parse(\"1\"); return n;",
            Code::TypeMismatch,
        );
        fails(
            "local o: option<u32> = u32.parse(n); return n;",
            Code::TypeMismatch,
        );
        fails(
            "local t: string = f64.to_string(1.0); return n;",
            Code::Syntax,
        );
        fails("return u32.wrapping_add(n);", Code::ArgCount);
        fails("return u32.wrapping_add(n, 1.5);", Code::TypeMismatch);
        fails(
            "local s: s8 = s8.wrapping_add(n, 1); return 0;",
            Code::TypeMismatch,
        );
        fails("return u32.checked_add(n, 1);", Code::TypeMismatch);
        fails("return u32.wrapping_from(300);", Code::UntypedLiteral);
        fails("return u32.wrapping_from(x);", Code::InvalidConversion);
        fails("return u32.wrapping_from(b);", Code::InvalidConversion);
    }

    #[test]
    fn errors() {
        fails("local x: u8 = 256; return 0;", Code::LiteralOutOfRange);
        fails("local x: s8 = 128; return 0;", Code::LiteralOutOfRange);
        fails("local x: s8 = -(128); return 0;", Code::LiteralOutOfRange);
        fails("local x: f64 = 1; return 0;", Code::TypeMismatch);
        fails("local y: u64 = n; return 0;", Code::TypeMismatch);
        fails("if (n) { } return 0;", Code::NonBoolCondition);
        fails("if (1) { } return 0;", Code::NonBoolCondition);
        fails("if (1 < 2) { } return 0;", Code::UntypedLiteral);
        fails("g = 3; return 0;", Code::AssignImmutable);
        fails("local g: u32 = 1; return 0;", Code::ShadowsGlobal);
        fails("local n: u32 = 1; return 0;", Code::DuplicateLocal);
        fails("return -n;", Code::InvalidOperator);
        fails("local r: f64 = x % x; return 0;", Code::InvalidOperator);
        fails("return 0 - 1 + b;", Code::TypeMismatch);
        fails("local z: u8 = u8(5); return 0;", Code::UntypedLiteral);
        fails("return u32(b);", Code::InvalidConversion);
        fails("break; return 0;", Code::BreakOutsideLoop);
        fails("return;", Code::MissingReturnValue);
        fails("if (b) { return 1; }", Code::MissingReturn);
        fails("while (true) { return 1; }", Code::MissingReturn);
        fails("return y;", Code::UnknownName);
        fails("return o();", Code::UnknownFunc);
        fails("return nope::o();", Code::UnknownName);
        fails("return f(1);", Code::ArgCount);
        fails("local a: u32 = v(); return 0;", Code::TypeMismatch);
        fails("return !n;", Code::InvalidOperator);
        fails("local q: bool = n && b; return 0;", Code::InvalidOperator);
        fails(
            "local q: u32 = 1; local q: u32 = 2; return 0;",
            Code::DuplicateLocal,
        );
        fails("local q: u32 = q; return 0;", Code::UnknownName);
    }

    #[test]
    fn sibling_blocks_reuse_names() {
        ok("if (b) { local t: u32 = 1; } else { local t: u32 = 2; } return 0;");
    }

    #[test]
    fn type_declarations() {
        let code = |t: &str| match analyze_str(t) {
            Ok(_) => panic!("accepted: {t}"),
            Err(e) => e.code,
        };
        let ok = |t: &str| {
            if let Err(e) = analyze_str(t) {
                panic!("{t}: {:?} {}", e.code, e.message);
            }
        };
        ok("interface i {
              record node { value: u32, children: list<node> }
              variant v { a(option<v>), b }
              record r { s: string, c: char, t: tuple<u8, list<mut r>>, o: option<i::r>, e: result<_, e>, f: f, x: result }
              enum e { a, b }
              flags f { a, b }
              func status() { }
              record status_line { status: e }
            }");
        assert_eq!(
            code("interface i { record line { a: u32 } func line() { } }"),
            Code::DuplicateName
        );
        assert_eq!(
            code("interface i { func line() { } enum line { a } }"),
            Code::DuplicateName
        );
        assert_eq!(
            code("interface i { enum e { a } flags e { a } }"),
            Code::DuplicateName
        );
        assert_eq!(
            code("interface i { func f() { } func f() { } }"),
            Code::DuplicateFunc
        );
        assert_eq!(
            code("interface i { record r { a: u32, a: u64 } }"),
            Code::DuplicateMember
        );
        assert_eq!(
            code("interface i { enum e { x, x } }"),
            Code::DuplicateMember
        );
        assert_eq!(
            code("interface i { variant v { x(u8), x } }"),
            Code::DuplicateMember
        );
        assert_eq!(
            code("interface math { record math { a: u32 } }"),
            Code::TypeNamedLikeTopLevel
        );
        assert_eq!(
            code("global g: u32 = 1; interface i { enum g { a } }"),
            Code::TypeNamedLikeTopLevel
        );
        let flags: Vec<String> = (0..33)
            .map(|i| format!("f{}", ["a", "b", "c"][i % 3].repeat(i / 3 + 1)))
            .collect();
        assert_eq!(
            code(&format!(
                "interface i {{ flags f {{ {} }} }}",
                flags.join(", ")
            )),
            Code::TooManyFlags
        );
        ok(&format!(
            "interface i {{ flags f {{ {} }} }}",
            flags[..32].join(", ")
        ));
        assert_eq!(
            code("interface i { record a { next: a } }"),
            Code::EmptyType
        );
        assert_eq!(
            code("interface i { record b { pair: tuple<u32, b> } }"),
            Code::EmptyType
        );
        // `result<a>` has the value `err`: `b` and `a` have values.
        ok("interface i { record a { b: b } record b { r: result<a> } }");
        assert_eq!(
            code("interface i { record a { b: b } record b { r: result<a, a> } }"),
            Code::EmptyType
        );
        // A variant has a value as soon as one of its cases has one (§ 7.9).
        ok("interface i { variant expr { lit(u32), neg(expr), add(tuple<expr, expr>) } }");
        ok("interface i { variant v { a(v), b } }");
        assert_eq!(
            code("interface i { variant v { a(v), b(tuple<u32, v>) } }"),
            Code::EmptyType
        );
        assert_eq!(
            code("interface i { variant v { a(w) } record w { v: v } }"),
            Code::EmptyType
        );
        let e = analyze_str("interface i { record node { value: u32, next: node } }").unwrap_err();
        assert!(
            e.message
                .contains("its field `next` of type `i::node` has no value"),
            "{}",
            e.message
        );
        assert_eq!(
            code("interface i { func f(x: nope) { } }"),
            Code::UnknownType
        );
        assert_eq!(
            code("interface i { func fib() { } func f(x: fib) { } }"),
            Code::UnknownType
        );
        assert_eq!(
            code("interface i { func f(x: j::t) { } } interface j { func t() { } }"),
            Code::UnknownType
        );
        assert_eq!(
            code("interface i { func f(x: j::t) { } } interface j { }"),
            Code::UnknownType
        );
        assert_eq!(
            code("interface i { func f(x: w::t) { } } world w { }"),
            Code::UnknownType
        );
        assert_eq!(
            code("interface i { func f(x: nope::t) { } }"),
            Code::UnknownName
        );
        assert_eq!(
            code("interface i { func f(x: t) { } } interface j { enum t { a } }"),
            Code::UnknownType
        );
        assert_eq!(
            code("interface i { enum t { a } } global g: t = 1;"),
            Code::UnknownType
        );
    }

    #[test]
    fn boundary_types() {
        let code = |t: &str| match analyze_str(t) {
            Ok(_) => panic!("accepted: {t}"),
            Err(e) => e.code,
        };
        let tree = "interface tree { record node { value: u32, children: list<node> } }";
        assert_eq!(
            code(&format!(
                "world w {{ export a; }} interface a {{ func root(n: tree::node) {{ }} }} {tree}"
            )),
            Code::RecursiveExport
        );
        assert_eq!(
            code(&format!(
                "world w {{ export a; }} interface a {{ func f(n: option<list<tree::node>>) {{ }} }} {tree}"
            )),
            Code::RecursiveExport
        );
        assert_eq!(
            code(&format!("world w {{ export tree; }} {tree}")),
            Code::RecursiveExport
        );
        // Inside a component, recursive types are fine.
        assert!(
            analyze_str(&format!(
                "world w {{ }} interface a {{ func root(n: tree::node) {{ }} }} {tree}"
            ))
            .is_ok()
        );

        let types = "interface types { enum color { red } }";
        let with_funcs = "interface paint { enum color { red } func f() { } }";
        assert!(
            analyze_str(&format!(
                "world w {{ export a; }} interface a {{ func f(c: types::color) {{ }} }} {types}"
            ))
            .is_ok()
        );
        assert_eq!(
            code(&format!(
                "world w {{ export a; }} interface a {{ func f(c: paint::color) {{ }} }} {with_funcs}"
            )),
            Code::UnexportedTypeInterface
        );
        assert_eq!(
            code(&format!(
                "world w {{ export a; }} interface a {{ record r {{ c: paint::color }} }} {with_funcs}"
            )),
            Code::UnexportedTypeInterface
        );
        assert!(analyze_str(&format!("world w {{ export a; export paint; }} interface a {{ func f(c: paint::color) {{ }} }} {with_funcs}")).is_ok());
    }

    #[test]
    fn use_cycles() {
        // `a` uses `b::s` in a type, `b` uses `a::r` in a function: a cycle in the WIT.
        let cycle = "interface a { record r { s: b::s } } \
                     interface b { record s { x: u32 } func f(r: a::r) { } }";
        let e = analyze_str(&format!("world w {{ export a; export b; }} {cycle}")).unwrap_err();
        assert_eq!(e.code, Code::UseCycle);
        assert!(
            e.message.contains("`a` uses `b::s`, `b` uses `a::r`"),
            "{}",
            e.message
        );
        // Through three interfaces, `c` only appearing because `b` uses its types.
        let three = "interface a { enum e { x } record r { s: b::s } } \
                     interface b { record s { t: c::t } func f(r: a::r) { } } \
                     interface c { record t { e: a::e } }";
        let e = analyze_str(&format!("world w {{ export a; export b; }} {three}")).unwrap_err();
        assert_eq!(e.code, Code::UseCycle);
        // No cycle between interfaces that do not appear in the WIT.
        assert!(analyze_str(cycle).is_ok());
    }

    #[test]
    fn declarations() {
        let code = |t: &str| analyze_str(t).unwrap_err().code;
        assert_eq!(
            code("interface a { } global a: u32 = 0;"),
            Code::DuplicateItem
        );
        assert_eq!(
            code("interface a { func f() { } func f() { } }"),
            Code::DuplicateFunc
        );
        assert_eq!(
            code("world w { export x; }"),
            Code::UnknownExportedInterface
        );
        assert_eq!(
            code("world w { export a; export a; } interface a { }"),
            Code::DuplicateExport
        );
        assert_eq!(code("global x: u8 = 300;"), Code::LiteralOutOfRange);
        assert_eq!(code("global x: u32 = -1;"), Code::InvalidOperator);
    }

    fn const_init(p: &hir::Program, i: usize) -> Const {
        match p.globals[i].init.kind {
            hir::ExprKind::Const(c) => c,
            _ => panic!("not a constant"),
        }
    }

    #[test]
    fn global_constants() {
        let p = analyze_str(
            "global a: s8 = -128; global b: u32 = 4294967295; global c: f64 = -1.5; global d: bool = true; global e: u64 = 18446744073709551615;",
        )
        .unwrap();
        assert_eq!(const_init(&p, 0), Const::I32(-128));
        assert_eq!(const_init(&p, 1), Const::I32(-1));
        assert_eq!(const_init(&p, 2), Const::F64(-1.5));
        assert_eq!(const_init(&p, 3), Const::I32(1));
        assert_eq!(const_init(&p, 4), Const::I64(-1));
    }
}
