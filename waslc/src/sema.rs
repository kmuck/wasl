//! Semantic analysis: name resolution, typing and control flow (§ 6 to 9, order of § 11.2).
//!
//! Turns the syntax trees of all the files of a package into a [`hir::Program`].

use std::collections::HashMap;

use crate::ast::{self, ExprKind, Ident, Item, UnaryOp};
use crate::diag::{Code, Result, err};
use crate::hir::{self, FuncId, InterfaceId};
use crate::types::{CaseDef, DeclId, DeclKind, FieldDef, Type, TypeDecl, TypeKind, Types};

mod body;

use body::{FnCx, always_returns};

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
        if !is_constant_hir(&init) {
            return err(
                Code::NonConstGlobal,
                global.init.span,
                "the initial value of a global must be a constant expression: a function call is not one",
            );
        }
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

/// Constant expression, the only initial value of a global (§ v2.4.12). Calls are
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::Const;
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
