//! Semantic analysis: name resolution, typing and control flow (§ 6 to 9, order of § 11.2).
//!
//! Turns the syntax trees of all the files of a package into a [`hir::Program`].

use std::collections::HashMap;

use crate::ast::{self, BinOp, ExprKind, Ident, Item, StmtKind, UnaryOp};
use crate::diag::{Code, Diagnostic, Result, err};
use crate::hir::{self, Const, FuncId, InterfaceId, LocalId, Place};
use crate::source::Span;
use crate::types::{CaseDef, DeclId, DeclKind, FieldDef, Type, TypeDecl, TypeKind, Types};

/// Maximum number of parameters of an exported function without linear memory (§ 6.5).
const MAX_EXPORTED_PARAMS: usize = 16;

/// Maximum number of flags of a flags type (§ v2.2.3).
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

/// Analyzes the package. Items must be given in the deterministic file order.
pub fn analyze(items: &[Item]) -> Result<hir::Program> {
    let mut worlds = Vec::new();
    let mut interfaces = Vec::new();
    let mut globals = Vec::new();
    let mut names: HashMap<&str, TopLevel> = HashMap::new();

    // 1. Top-level names (§ 6.1).
    for item in items {
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
                globals.push(g);
                TopLevel::Global
            }
        };
        names.insert(&name.name, entry);
    }

    // 2. Items of the interfaces (§ 6.3, § v2.2.3).
    let mut program = hir::Program::default();
    let mut types = Types::default();
    types.interface_names = interfaces.iter().map(|i| i.name.name.clone()).collect();
    let mut funcs: Vec<&ast::Func> = Vec::new();
    let mut decls: Vec<&ast::TypeDecl> = Vec::new();
    let mut scopes: Vec<InterfaceScope> = Vec::new();
    for (iface_id, iface) in interfaces.iter().enumerate() {
        let mut scope = InterfaceScope::default();
        let mut func_ids = Vec::new();
        let mut decl_ids = Vec::new();
        for item in &iface.items {
            let name = match item {
                ast::InterfaceItem::Func(f) => &f.name,
                ast::InterfaceItem::Type(t) => &t.name,
            };
            let previous = if scope.funcs.contains_key(name.name.as_str()) {
                Some("a function")
            } else if scope.types.contains_key(name.name.as_str()) {
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
                            "`{}` is already declared as {previous} in interface `{}`: types and functions of an interface share one namespace",
                            name.name, iface.name.name
                        ),
                    );
                }
                (_, None) => {}
            }
            match item {
                ast::InterfaceItem::Func(func) => {
                    let id = funcs.len();
                    funcs.push(func);
                    scope.funcs.insert(&func.name.name, id);
                    func_ids.push(id);
                }
                ast::InterfaceItem::Type(decl) => {
                    if let Some(top) = names.get(decl.name.name.as_str()) {
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
                    let id = decls.len();
                    decls.push(decl);
                    scope.types.insert(&decl.name.name, id);
                    decl_ids.push(id);
                    // The members are resolved in step 3; the names are known now.
                    types.decls.push(TypeDecl {
                        name: decl.name.name.clone(),
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
                    types.decl_type(id);
                }
            }
        }
        scopes.push(scope);
        program.interfaces.push(hir::InterfaceDef {
            name: iface.name.name.clone(),
            funcs: func_ids,
            types: decl_ids,
        });
    }

    // 3. Types: fields, payloads and signatures, interface by interface (§ v2.8.1).
    let resolver = Resolver {
        names: &names,
        scopes: &scopes,
    };
    for (iface_id, iface) in interfaces.iter().enumerate() {
        for item in &iface.items {
            match item {
                ast::InterfaceItem::Type(decl) => {
                    let id = scopes[iface_id].types[decl.name.name.as_str()];
                    match &decl.kind {
                        ast::TypeDeclKind::Record(fields) => {
                            let mut defs = Vec::new();
                            for f in fields {
                                defs.push(FieldDef {
                                    name: f.name.name.clone(),
                                    mutable: f.mutable,
                                    ty: resolver.resolve(&mut types, &f.ty, Some(iface_id))?,
                                });
                            }
                            types.decls[id].kind = DeclKind::Record(defs);
                        }
                        ast::TypeDeclKind::Variant(cases) => {
                            let mut defs = Vec::new();
                            for c in cases {
                                let payload = match &c.payload {
                                    Some(ty) => {
                                        Some(resolver.resolve(&mut types, ty, Some(iface_id))?)
                                    }
                                    None => None,
                                };
                                defs.push(CaseDef {
                                    name: c.name.name.clone(),
                                    payload,
                                });
                            }
                            types.decls[id].kind = DeclKind::Variant(defs);
                        }
                        ast::TypeDeclKind::Enum(_) | ast::TypeDeclKind::Flags(_) => {}
                    }
                }
                ast::InterfaceItem::Func(func) => {
                    let mut params = Vec::new();
                    for p in &func.params {
                        params.push((
                            p.name.name.clone(),
                            resolver.resolve(&mut types, &p.ty, Some(iface_id))?,
                        ));
                    }
                    let ret = match &func.ret {
                        Some(ty) => Some(resolver.resolve(&mut types, ty, Some(iface_id))?),
                        None => None,
                    };
                    program.funcs.push(hir::FuncDef {
                        name: func.name.name.clone(),
                        interface: iface_id,
                        params,
                        ret,
                        locals: Vec::new(),
                        body: Vec::new(),
                    });
                }
            }
        }
    }
    // Functions were pushed in interface order, which is the order of `funcs`.
    for (id, decl) in decls.iter().enumerate() {
        if let Some(path) = empty_cycle(&types, id) {
            return err(
                Code::EmptyType,
                decl.name.span,
                format!(
                    "type `{}` has no value: it contains itself through {path} without going through an `option` or a `list`",
                    decl.name.name
                ),
            );
        }
    }

    // 3. Worlds (§ 6.2).
    for world in &worlds {
        let mut exports = Vec::new();
        for export in &world.exports {
            let id = match names.get(export.name.as_str()) {
                Some(TopLevel::Interface(id)) => *id,
                Some(other) => {
                    return err(
                        Code::UnknownExportedInterface,
                        export.span,
                        format!(
                            "`{}` is {}, not an interface",
                            export.name,
                            other.describe()
                        ),
                    );
                }
                None => {
                    return err(
                        Code::UnknownExportedInterface,
                        export.span,
                        format!("unknown interface `{}`", export.name),
                    );
                }
            };
            if exports.contains(&id) {
                return err(
                    Code::DuplicateExport,
                    export.span,
                    format!(
                        "interface `{}` is already exported by world `{}`",
                        export.name, world.name.name
                    ),
                );
            }
            exports.push(id);
        }
        program.worlds.push(hir::WorldDef {
            name: world.name.name.clone(),
            exports,
        });
    }

    // 4. Exported functions: parameter limit (§ 6.5) and types that can cross the
    //    boundary (§ v2.2.7).
    for world in &program.worlds {
        for &iface_id in &world.exports {
            let iface = interfaces[iface_id];
            for func in iface.funcs() {
                if func.params.len() > MAX_EXPORTED_PARAMS {
                    return err(
                        Code::TooManyParams,
                        func.name.span,
                        format!(
                            "exported function `{}` has {} parameters, the maximum is {MAX_EXPORTED_PARAMS}",
                            func.name.name,
                            func.params.len()
                        ),
                    );
                }
                let def = &program.funcs[scopes[iface_id].funcs[func.name.name.as_str()]];
                let signature: Vec<Type> =
                    def.params.iter().map(|(_, t)| *t).chain(def.ret).collect();
                check_boundary_types(
                    &types,
                    &program,
                    world,
                    &signature,
                    &func.name,
                    "exported function",
                )?;
            }
            // The types of an exported interface all appear in the WIT (§ v2.7.1).
            for decl in iface.types() {
                let ty = types.decl_type_of(scopes[iface_id].types[decl.name.name.as_str()]);
                check_boundary_types(
                    &types,
                    &program,
                    world,
                    &[ty],
                    &decl.name,
                    "type of an exported interface",
                )?;
            }
        }
    }

    // 5. Globals (§ 6.4, § v2.4.12).
    let mut global_ids = HashMap::new();
    for global in &globals {
        let ty = resolver.resolve(&mut types, &global.ty, None)?;
        if !is_constant(&global.init) {
            return err(
                Code::NonConstGlobal,
                global.init.span,
                "the initial value of a global must be a constant expression, such as a literal",
            );
        }
        let mut cx = FnCx::new(&program, &mut types, &resolver, &global_ids, None, None);
        let init = cx.expr(&global.init, Some(ty))?;
        let hir::ExprKind::Const(init) = init.kind else {
            unreachable!("a constant expression evaluates to a constant")
        };
        global_ids.insert(global.name.name.as_str(), program.globals.len());
        program.globals.push(hir::GlobalDef {
            name: global.name.name.clone(),
            ty,
            mutable: global.mutable,
            init,
        });
    }

    // 6. Function bodies.
    for (id, func) in funcs.iter().enumerate() {
        let iface = program.funcs[id].interface;
        let ret = program.funcs[id].ret;
        let mut cx = FnCx::new(
            &program,
            &mut types,
            &resolver,
            &global_ids,
            Some(iface),
            ret,
        );
        cx.scopes.push(Vec::new());
        for (param, (_, ty)) in func.params.iter().zip(&program.funcs[id].params) {
            cx.declare(&param.name, *ty)?;
        }
        let body = cx.block(&func.body)?;
        if func.ret.is_some() && !always_returns(&body) {
            return err(
                Code::MissingReturn,
                func.name.span,
                format!(
                    "function `{}` does not end with a `return` on every path",
                    func.name.name
                ),
            );
        }
        let locals = cx.locals;
        let def = &mut program.funcs[id];
        def.locals = locals;
        def.body = body;
    }

    program.types = types;
    Ok(program)
}

fn unsupported<T>(span: Span, what: &str) -> Result<T> {
    err(
        Code::Unsupported,
        span,
        format!("{what} are not implemented yet"),
    )
}

/// Constant expression, the only initial value of a global (§ v2.4.12).
fn is_constant(e: &ast::Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) => true,
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => matches!(expr.kind, ExprKind::Int(_) | ExprKind::Float(_)),
        _ => false,
    }
}

/// Functions and types of an interface, by name.
#[derive(Default)]
struct InterfaceScope<'a> {
    funcs: HashMap<&'a str, FuncId>,
    types: HashMap<&'a str, DeclId>,
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

/// A cycle of types that does not go through an `option` or a `list` (§ v2.2.5): returns
/// the path of the cycle, from the declared type back to itself.
fn empty_cycle(types: &Types, id: DeclId) -> Option<String> {
    fn visit(
        types: &Types,
        ty: Type,
        target: DeclId,
        path: &mut Vec<Type>,
        seen: &mut Vec<Type>,
    ) -> bool {
        let children = match types.kind(ty) {
            // A list or an option can be empty: the cycle stops here.
            TypeKind::List { .. } | TypeKind::Option(_) => return false,
            _ => types.children(ty),
        };
        for child in children {
            if matches!(types.kind(child), TypeKind::Decl(d) if *d == target) {
                path.push(child);
                return true;
            }
            if seen.contains(&child) {
                continue;
            }
            seen.push(child);
            path.push(child);
            if visit(types, child, target, path, seen) {
                return true;
            }
            path.pop();
        }
        false
    }
    let start = types.decl_type_of(id);
    let mut path = Vec::new();
    if !visit(types, start, id, &mut path, &mut Vec::new()) {
        return None;
    }
    let decls: Vec<String> = path
        .iter()
        .filter_map(|&t| types.decl(t).map(|(_, d)| format!("`{}`", d.name)))
        .collect();
    Some(decls.join(" → "))
}

/// Types used at the boundary of a component by `world` (§ v2.2.7): no recursive type
/// (E0208), and only types of interfaces that the world exports or that have no
/// function (E0209).
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
            if !world.exports.contains(&decl.interface) && !iface.funcs.is_empty() {
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
    Ok(())
}

/// Resolves the types written in the source (§ v2.2.4).
struct Resolver<'a> {
    names: &'a HashMap<&'a str, TopLevel>,
    scopes: &'a [InterfaceScope<'a>],
}

impl Resolver<'_> {
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
            K::Named(name) => TypeKind::Decl(self.decl(name, iface)?),
        };
        Ok(types.intern(kind))
    }

    /// The declared type named `name`.
    fn decl(&self, name: &ast::TypeName, current: Option<InterfaceId>) -> Result<DeclId> {
        let unknown = |message: String| err(Code::UnknownType, name.span(), message);
        let iface = match &name.iface {
            Some(iface) => match self.names.get(iface.name.as_str()) {
                Some(TopLevel::Interface(id)) => *id,
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
                        format!("unknown interface `{}`", iface.name),
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
                                Some(*n)
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
                        Some(*n)
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

/// A block always returns if one of its statements always returns (§ 8.8).
fn always_returns(stmts: &[hir::Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        hir::Stmt::Return(_) => true,
        hir::Stmt::If { then, else_, .. } => always_returns(then) && always_returns(else_),
        _ => false,
    })
}

/// Expression made only of numeric literals: its type comes from the context (§ 8.2).
fn needs_type_context(e: &ast::Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) => true,
        ExprKind::Paren(inner) => needs_type_context(inner),
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => needs_type_context(expr),
        ExprKind::Binary { op, lhs, rhs } if !op.is_comparison() && !is_logical(*op) => {
            needs_type_context(lhs) && needs_type_context(rhs)
        }
        _ => false,
    }
}

fn is_logical(op: BinOp) -> bool {
    matches!(op, BinOp::And | BinOp::Or)
}

/// Analysis context of a function (or of the initial value of a global).
struct FnCx<'a> {
    program: &'a hir::Program,
    types: &'a mut Types,
    resolver: &'a Resolver<'a>,
    globals: &'a HashMap<&'a str, hir::GlobalId>,
    /// Interface of the function; `None` for the initial value of a global.
    iface: Option<InterfaceId>,
    ret: Option<Type>,
    locals: Vec<Type>,
    /// Nested scopes: visible names and their variable.
    scopes: Vec<Vec<(String, LocalId)>>,
    loop_depth: u32,
}

impl<'a> FnCx<'a> {
    fn new(
        program: &'a hir::Program,
        types: &'a mut Types,
        resolver: &'a Resolver<'a>,
        globals: &'a HashMap<&'a str, hir::GlobalId>,
        iface: Option<InterfaceId>,
        ret: Option<Type>,
    ) -> Self {
        FnCx {
            program,
            types,
            resolver,
            globals,
            iface,
            ret,
            locals: Vec::new(),
            scopes: Vec::new(),
            loop_depth: 0,
        }
    }

    /// A type, as written in messages.
    fn show(&self, ty: Type) -> String {
        self.types.display(ty)
    }

    fn mismatch<T>(&self, span: Span, expected: Type, found: Type) -> Result<T> {
        err(
            Code::TypeMismatch,
            span,
            format!(
                "mismatched types: expected `{}`, found `{}`",
                self.show(expected),
                self.show(found)
            ),
        )
    }

    /// Type of a local, which can only be a primitive type for now.
    fn local_type(&mut self, ty: &ast::TypeExpr) -> Result<Type> {
        let resolved = self.resolver.resolve(self.types, ty, self.iface)?;
        if resolved.as_prim().is_none() {
            return unsupported(ty.span, "locals of types other than numbers and `bool`");
        }
        Ok(resolved)
    }

    // ---- Variables ----

    fn lookup_local(&self, name: &str) -> Option<LocalId> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }

    fn declare(&mut self, name: &Ident, ty: Type) -> Result<LocalId> {
        if matches!(
            self.resolver.names.get(name.name.as_str()),
            Some(TopLevel::Global)
        ) {
            return err(
                Code::ShadowsGlobal,
                name.span,
                format!("`{}` is already the name of a global", name.name),
            );
        }
        if self.lookup_local(&name.name).is_some() {
            return err(
                Code::DuplicateLocal,
                name.span,
                format!("`{}` is already declared in this function", name.name),
            );
        }
        let id = self.locals.len() as LocalId;
        self.locals.push(ty);
        self.scopes
            .last_mut()
            .expect("open scope")
            .push((name.name.clone(), id));
        Ok(id)
    }

    // ---- Statements ----

    fn block(&mut self, block: &ast::Block) -> Result<Vec<hir::Stmt>> {
        self.scopes.push(Vec::new());
        let mut stmts = Vec::with_capacity(block.stmts.len());
        for stmt in &block.stmts {
            stmts.push(self.stmt(stmt)?);
        }
        self.scopes.pop();
        Ok(stmts)
    }

    fn stmt(&mut self, stmt: &ast::Stmt) -> Result<hir::Stmt> {
        Ok(match &stmt.kind {
            StmtKind::Local { name, ty, init } => {
                let ty = self.local_type(ty)?;
                // The initial value does not see the local yet (§ 9.4).
                let init = match init {
                    Some(e) => Some(self.expr(e, Some(ty))?),
                    None => None,
                };
                let local = self.declare(name, ty)?;
                hir::Stmt::Local { local, init }
            }
            StmtKind::Assign { target, value } => {
                if !target.selectors.is_empty() {
                    return unsupported(stmt.span, "assignments to fields and elements");
                }
                let target = &target.name;
                let (place, ty) = if let Some(id) = self.lookup_local(&target.name) {
                    (Place::Local(id), self.locals[id as usize])
                } else if let Some(&id) = self.globals.get(target.name.as_str()) {
                    let global = &self.program.globals[id];
                    if !global.mutable {
                        return err(
                            Code::AssignImmutable,
                            target.span,
                            format!(
                                "global `{}` is not mutable: declare it as `global {}: mut {}`",
                                global.name,
                                global.name,
                                self.show(global.ty)
                            ),
                        );
                    }
                    (Place::Global(id), global.ty)
                } else {
                    return self.unknown_name(target);
                };
                let value = self.expr(value, Some(ty))?;
                hir::Stmt::Assign { place, value }
            }
            StmtKind::If(if_) => self.if_(if_)?,
            StmtKind::Match(_) => return unsupported(stmt.span, "`match` statements"),
            StmtKind::While { cond, body } => {
                let cond = self.condition(cond)?;
                self.loop_depth += 1;
                let body = self.block(body)?;
                self.loop_depth -= 1;
                hir::Stmt::While { cond, body }
            }
            StmtKind::Break | StmtKind::Continue => {
                if self.loop_depth == 0 {
                    let kw = if matches!(stmt.kind, StmtKind::Break) {
                        "break"
                    } else {
                        "continue"
                    };
                    return err(
                        Code::BreakOutsideLoop,
                        stmt.span,
                        format!("`{kw}` can only be used inside a `while`"),
                    );
                }
                if matches!(stmt.kind, StmtKind::Break) {
                    hir::Stmt::Break
                } else {
                    hir::Stmt::Continue
                }
            }
            StmtKind::Return(value) => match (value, self.ret) {
                (None, None) => hir::Stmt::Return(None),
                (Some(e), Some(ty)) => hir::Stmt::Return(Some(self.expr(e, Some(ty))?)),
                (None, Some(ty)) => {
                    return err(
                        Code::MissingReturnValue,
                        stmt.span,
                        format!("`return` must return a value of type `{}`", self.show(ty)),
                    );
                }
                (Some(e), None) => {
                    return err(
                        Code::UnexpectedReturnValue,
                        e.span,
                        "this function does not return a value: write `return;`",
                    );
                }
            },
            StmtKind::Call(call) => hir::Stmt::Call(self.call(call)?),
        })
    }

    fn if_(&mut self, if_: &ast::If) -> Result<hir::Stmt> {
        let cond = self.condition(&if_.cond)?;
        let then = self.block(&if_.then)?;
        let else_ = match &if_.else_ {
            None => Vec::new(),
            Some(ast::Else::If(nested)) => vec![self.if_(nested)?],
            Some(ast::Else::Block(block)) => self.block(block)?,
        };
        Ok(hir::Stmt::If { cond, then, else_ })
    }

    fn condition(&mut self, cond: &ast::Expr) -> Result<hir::Expr> {
        let non_bool = |found: &dyn std::fmt::Display| {
            Diagnostic::new(
                Code::NonBoolCondition,
                cond.span,
                format!(
                    "the condition must be of type `bool`, found {found} (there is no implicit conversion: write for example `x != 0`)"
                ),
            )
        };
        if needs_type_context(cond) {
            return Err(non_bool(&"a numeric literal"));
        }
        let e = self.expr(cond, None)?;
        let ty = e.value_ty();
        if ty != Type::BOOL {
            return Err(non_bool(&format!("`{}`", self.show(ty))));
        }
        Ok(e)
    }

    // ---- Expressions ----

    /// Analyzes an expression used as a value. `expected` is the type expected by the
    /// context, if known; it gives literals their type (§ 8.2).
    fn expr(&mut self, e: &ast::Expr, expected: Option<Type>) -> Result<hir::Expr> {
        let result = match &e.kind {
            ExprKind::Int(v) => return self.int_literal(*v, false, e.span, expected),
            ExprKind::Float(s) => return self.float_literal(s, e.span, expected),
            ExprKind::Bool(b) => typed(hir::ExprKind::Const(Const::bool(*b)), Type::BOOL),
            ExprKind::Paren(inner) => return self.expr(inner, expected),
            ExprKind::Name(name) => {
                if let Some(id) = self.lookup_local(name) {
                    typed(hir::ExprKind::Local(id), self.locals[id as usize])
                } else if let Some(&id) = self.globals.get(name.as_str()) {
                    typed(hir::ExprKind::Global(id), self.program.globals[id].ty)
                } else {
                    return self.unknown_name(&Ident {
                        name: name.clone(),
                        span: e.span,
                    });
                }
            }
            ExprKind::Call(call) => {
                let result = self.call(call)?;
                if result.ty.is_none() {
                    return err(
                        Code::TypeMismatch,
                        e.span,
                        format!("function `{}` does not return a value", call.name.name),
                    );
                }
                result
            }
            ExprKind::Convert { ty, arg } => self.convert(ty, arg)?,
            ExprKind::Str(_) => return unsupported(e.span, "string literals"),
            ExprKind::Char(_) => return unsupported(e.span, "char literals"),
            ExprKind::Path(_) => return unsupported(e.span, "paths to cases"),
            ExprKind::Record { .. } => return unsupported(e.span, "record literals"),
            ExprKind::Flags { .. } => return unsupported(e.span, "flags literals"),
            ExprKind::List(_) => return unsupported(e.span, "list literals"),
            ExprKind::Tuple(_) => return unsupported(e.span, "tuples"),
            ExprKind::Field { .. } | ExprKind::Elem { .. } => {
                return unsupported(e.span, "field accesses");
            }
            ExprKind::Index { .. } => return unsupported(e.span, "indexes"),
            ExprKind::Unary { op, expr } => return self.unary(*op, expr, e.span, expected),
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, e.span, expected)?,
        };
        let ty = result.value_ty();
        match expected {
            Some(exp) if exp != ty => self.mismatch(e.span, exp, ty),
            _ => Ok(result),
        }
    }

    fn unknown_name<T>(&self, name: &Ident) -> Result<T> {
        err(
            Code::UnknownName,
            name.span,
            format!("unknown name `{}`", name.name),
        )
    }

    fn int_literal(
        &self,
        value: u128,
        negative: bool,
        span: Span,
        expected: Option<Type>,
    ) -> Result<hir::Expr> {
        let ty = match expected {
            None => return untyped_literal(span),
            Some(ty) if ty.is_float() => {
                return err(
                    Code::TypeMismatch,
                    span,
                    format!(
                        "integer literal used for the float type `{}`: write for example `{}.0`",
                        self.show(ty),
                        if negative {
                            format!("-{value}")
                        } else {
                            value.to_string()
                        }
                    ),
                );
            }
            Some(Type::BOOL) => {
                return err(
                    Code::TypeMismatch,
                    span,
                    "integer literal used for type `bool`: write `true` or `false`",
                );
            }
            Some(ty) => ty,
        };
        if negative && ty.is_unsigned() {
            return err(
                Code::InvalidOperator,
                span,
                format!(
                    "`-` cannot be applied to the unsigned type `{}`",
                    self.show(ty)
                ),
            );
        }
        let (min, max) = ty.int_range();
        let value = i128::try_from(value)
            .ok()
            .map(|v| if negative { -v } else { v });
        match value {
            Some(v) if v >= min && v <= max => {
                Ok(typed(hir::ExprKind::Const(Const::int(ty, v)), ty))
            }
            _ => err(
                Code::LiteralOutOfRange,
                span,
                format!(
                    "literal out of range for type `{}` ({min} … {max})",
                    self.show(ty)
                ),
            ),
        }
    }

    fn float_literal(&self, text: &str, span: Span, expected: Option<Type>) -> Result<hir::Expr> {
        let (konst, ty) = match expected {
            None => return untyped_literal(span),
            Some(Type::F32) => {
                let v: f32 = text.parse().expect("valid float literal");
                (Const::F32(v), Type::F32)
            }
            Some(Type::F64) => {
                let v: f64 = text.parse().expect("valid float literal");
                (Const::F64(v), Type::F64)
            }
            Some(ty) => {
                return err(
                    Code::TypeMismatch,
                    span,
                    format!("float literal used for type `{}`", self.show(ty)),
                );
            }
        };
        let infinite = match konst {
            Const::F32(v) => v.is_infinite(),
            Const::F64(v) => v.is_infinite(),
            _ => false,
        };
        if infinite {
            return err(
                Code::LiteralOutOfRange,
                span,
                format!("literal out of range for type `{}`", self.show(ty)),
            );
        }
        Ok(typed(hir::ExprKind::Const(konst), ty))
    }

    fn unary(
        &mut self,
        op: UnaryOp,
        operand: &ast::Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Result<hir::Expr> {
        match op {
            UnaryOp::Neg => {
                // `-` applied directly to an integer literal: negative literal (§ 8.2).
                if let ExprKind::Int(v) = operand.kind {
                    return self.int_literal(v, true, span, expected);
                }
                let inner = self.expr(operand, expected)?;
                let ty = inner.value_ty();
                if !(ty.is_signed() || ty.is_float()) {
                    return err(
                        Code::InvalidOperator,
                        span,
                        format!("`-` cannot be applied to type `{}`", self.show(ty)),
                    );
                }
                // Constant folding: the initial value of a global must stay constant.
                let kind = match inner.kind {
                    hir::ExprKind::Const(Const::F32(v)) => hir::ExprKind::Const(Const::F32(-v)),
                    hir::ExprKind::Const(Const::F64(v)) => hir::ExprKind::Const(Const::F64(-v)),
                    hir::ExprKind::Const(Const::I32(v)) => {
                        hir::ExprKind::Const(Const::int(ty, -i128::from(v)))
                    }
                    hir::ExprKind::Const(Const::I64(v)) => {
                        hir::ExprKind::Const(Const::int(ty, -i128::from(v)))
                    }
                    other => hir::ExprKind::Unary {
                        op,
                        expr: Box::new(hir::Expr {
                            kind: other,
                            ty: Some(ty),
                        }),
                    },
                };
                Ok(typed(kind, ty))
            }
            UnaryOp::Not => {
                let inner = self.expr(operand, None)?;
                let ty = inner.value_ty();
                if ty != Type::BOOL {
                    return err(
                        Code::InvalidOperator,
                        span,
                        format!("`!` only applies to type `bool`, found `{}`", self.show(ty)),
                    );
                }
                if let Some(exp) = expected
                    && exp != Type::BOOL
                {
                    return self.mismatch(span, exp, Type::BOOL);
                }
                Ok(typed(
                    hir::ExprKind::Unary {
                        op,
                        expr: Box::new(inner),
                    },
                    Type::BOOL,
                ))
            }
        }
    }

    fn binary(
        &mut self,
        op: BinOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        span: Span,
        expected: Option<Type>,
    ) -> Result<hir::Expr> {
        if is_logical(op) {
            let l = self.logical_operand(op, lhs)?;
            let r = self.logical_operand(op, rhs)?;
            return Ok(typed(
                hir::ExprKind::Binary {
                    op,
                    lhs: Box::new(l),
                    rhs: Box::new(r),
                },
                Type::BOOL,
            ));
        }

        // Operand type: the type of the operand that has its own type, otherwise the type
        // expected by the context (for operators whose result has the operand type).
        let (l, r) = if !needs_type_context(lhs) {
            let l = self.expr(lhs, None)?;
            let r = self.expr(rhs, Some(l.value_ty()))?;
            (l, r)
        } else if !needs_type_context(rhs) {
            let r = self.expr(rhs, None)?;
            let l = self.expr(lhs, Some(r.value_ty()))?;
            (l, r)
        } else {
            match expected.filter(|_| !op.is_comparison()) {
                Some(ty) => (self.expr(lhs, Some(ty))?, self.expr(rhs, Some(ty))?),
                None => return untyped_literal(lhs.span),
            }
        };

        let ty = l.value_ty();
        let allowed = match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => ty.is_numeric(),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => ty.is_numeric(),
            BinOp::Rem | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                ty.is_int()
            }
            BinOp::Eq | BinOp::Ne => true,
            BinOp::And | BinOp::Or => unreachable!(),
        };
        if !allowed {
            let hint = if op == BinOp::Rem && ty.is_float() {
                " (Wasm has no float remainder)"
            } else {
                ""
            };
            return err(
                Code::InvalidOperator,
                span,
                format!(
                    "operator `{}` cannot be applied to type `{}`{hint}",
                    op.symbol(),
                    self.show(ty)
                ),
            );
        }
        let result_ty = if op.is_comparison() { Type::BOOL } else { ty };
        Ok(typed(
            hir::ExprKind::Binary {
                op,
                lhs: Box::new(l),
                rhs: Box::new(r),
            },
            result_ty,
        ))
    }

    fn logical_operand(&mut self, op: BinOp, e: &ast::Expr) -> Result<hir::Expr> {
        let operand = self.expr(e, None)?;
        let ty = operand.value_ty();
        if ty != Type::BOOL {
            return err(
                Code::InvalidOperator,
                e.span,
                format!(
                    "operator `{}` only applies to type `bool`, found `{}`",
                    op.symbol(),
                    self.show(ty)
                ),
            );
        }
        Ok(operand)
    }

    fn convert(&mut self, target: &ast::TypeExpr, arg: &ast::Expr) -> Result<hir::Expr> {
        let target = match target.kind {
            ast::TypeExprKind::Prim(t) => t,
            _ => {
                return err(
                    Code::InvalidConversion,
                    target.span,
                    "only numeric types have conversions; a number becomes a `char` with `char::from_u32`",
                );
            }
        };
        if needs_type_context(arg) {
            return err(
                Code::UntypedLiteral,
                arg.span,
                format!(
                    "cannot infer the type of the literal: there is no need to convert a literal, write a value of type `{target}` directly"
                ),
            );
        }
        let inner = self.expr(arg, None)?;
        let source = inner.value_ty();
        if !source.is_numeric() || !target.is_numeric() {
            return err(
                Code::InvalidConversion,
                arg.span,
                format!(
                    "cannot convert `{}` to `{target}`: only conversions between numeric types exist",
                    self.show(source)
                ),
            );
        }
        Ok(typed(
            hir::ExprKind::Convert(Box::new(inner)),
            Type::prim(target),
        ))
    }

    /// Analyzes a call. The result has `ty: None` if the function returns nothing.
    fn call(&mut self, call: &ast::Call) -> Result<hir::Expr> {
        let iface = match call.path.as_slice() {
            [] => self.iface.expect("calls are in functions"),
            [name]
                if matches!(
                    name.name.as_str(),
                    "string" | "list" | "char" | "option" | "result"
                ) =>
            {
                return unsupported(
                    name.span.to(call.name.span),
                    "built-in functions and constructors",
                );
            }
            [first, ..] if call.path.len() > 1 => {
                return unsupported(first.span.to(call.name.span), "constructors");
            }
            [.., name] => match self.resolver.names.get(name.name.as_str()) {
                Some(TopLevel::Interface(id)) => *id,
                Some(other) => {
                    return err(
                        Code::UnknownName,
                        name.span,
                        format!("`{}` is {}, not an interface", name.name, other.describe()),
                    );
                }
                None => {
                    return err(
                        Code::UnknownName,
                        name.span,
                        format!("unknown interface `{}`", name.name),
                    );
                }
            },
        };
        let iface_name = &self.program.interfaces[iface].name;
        let scopes = self.resolver.scopes;
        let Some(&func_id) = scopes[iface].funcs.get(call.name.name.as_str()) else {
            let mut message = format!(
                "interface `{iface_name}` has no function `{}`",
                call.name.name
            );
            if call.path.is_empty()
                && let Some(other) = scopes
                    .iter()
                    .position(|s| s.funcs.contains_key(call.name.name.as_str()))
            {
                message += &format!(
                    "; to call the one from another interface, write `{}::{}(…)`",
                    self.program.interfaces[other].name, call.name.name
                );
            }
            return err(Code::UnknownFunc, call.name.span, message);
        };
        let func = &self.program.funcs[func_id];
        if func.params.len() != call.args.len() {
            return err(
                Code::ArgCount,
                call.name.span,
                format!(
                    "`{}` expects {} argument(s), {} given",
                    func.name,
                    func.params.len(),
                    call.args.len()
                ),
            );
        }
        let param_types: Vec<Type> = func.params.iter().map(|(_, ty)| *ty).collect();
        let ret = func.ret;
        let mut args = Vec::with_capacity(call.args.len());
        for (arg, ty) in call.args.iter().zip(param_types) {
            args.push(self.expr(arg, Some(ty))?);
        }
        Ok(hir::Expr {
            kind: hir::ExprKind::Call {
                func: func_id,
                args,
            },
            ty: ret,
        })
    }
}

fn typed(kind: hir::ExprKind, ty: Type) -> hir::Expr {
    hir::Expr { kind, ty: Some(ty) }
}

fn untyped_literal<T>(span: Span) -> Result<T> {
    err(
        Code::UntypedLiteral,
        span,
        "cannot infer the type of the literal: nothing in the context gives its type",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::source::FileId;

    fn analyze_str(text: &str) -> Result<hir::Program> {
        analyze(&parse(tokenize(FileId(0), text)?)?)
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
        assert_eq!(
            code("interface i { record a { b: b } record b { r: result<a> } }"),
            Code::EmptyType
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
        let params = (0..17)
            .map(|i| {
                format!(
                    "p{}: u32",
                    [
                        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o",
                        "p", "q"
                    ][i]
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(
            code(&format!(
                "world w {{ export a; }} interface a {{ func f({params}) {{ }} }}"
            )),
            Code::TooManyParams
        );
        // An internal interface may exceed the limit.
        assert!(analyze_str(&format!("interface a {{ func f({params}) {{ }} }}")).is_ok());
    }

    #[test]
    fn global_constants() {
        let p = analyze_str(
            "global a: s8 = -128; global b: u32 = 4294967295; global c: f64 = -1.5; global d: bool = true; global e: u64 = 18446744073709551615;",
        )
        .unwrap();
        assert_eq!(p.globals[0].init, Const::I32(-128));
        assert_eq!(p.globals[1].init, Const::I32(-1));
        assert_eq!(p.globals[2].init, Const::F64(-1.5));
        assert_eq!(p.globals[3].init, Const::I32(1));
        assert_eq!(p.globals[4].init, Const::I64(-1));
    }
}
