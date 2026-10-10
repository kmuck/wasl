//! Semantic analysis: name resolution, typing and control flow (§ 6 to 9, order of § 11.2).
//!
//! Turns the syntax trees of all the files of a package into a [`hir::Program`].

use std::collections::HashMap;

use crate::ast::{self, BinOp, ExprKind, Ident, Item, StmtKind, UnaryOp};
use crate::diag::{Code, Diagnostic, Result, err};
use crate::hir::{self, Const, FuncId, InterfaceId, LocalId, Place};
use crate::source::Span;
use crate::types::Type;

/// Maximum number of parameters of an exported function without linear memory (§ 6.5).
const MAX_EXPORTED_PARAMS: usize = 16;

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

    // 2. Interface functions (§ 6.3).
    let mut program = hir::Program::default();
    let mut funcs: Vec<&ast::Func> = Vec::new();
    let mut func_names: Vec<HashMap<&str, FuncId>> = Vec::new();
    for (iface_id, iface) in interfaces.iter().enumerate() {
        let mut by_name = HashMap::new();
        let mut ids = Vec::new();
        for func in &iface.funcs {
            if by_name.contains_key(func.name.name.as_str()) {
                return err(
                    Code::DuplicateFunc,
                    func.name.span,
                    format!(
                        "function `{}` is already declared in interface `{}`",
                        func.name.name, iface.name.name
                    ),
                );
            }
            let id = funcs.len();
            funcs.push(func);
            by_name.insert(func.name.name.as_str(), id);
            ids.push(id);
            program.funcs.push(hir::FuncDef {
                name: func.name.name.clone(),
                interface: iface_id,
                params: func
                    .params
                    .iter()
                    .map(|p| (p.name.name.clone(), p.ty))
                    .collect(),
                ret: func.ret,
                locals: Vec::new(),
                body: Vec::new(),
            });
        }
        func_names.push(by_name);
        program.interfaces.push(hir::InterfaceDef {
            name: iface.name.name.clone(),
            funcs: ids,
        });
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

    // 4. Parameter limit of exported functions (§ 6.5).
    for (iface_id, iface) in interfaces.iter().enumerate() {
        if !program.is_exported(iface_id) {
            continue;
        }
        for func in &iface.funcs {
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
        }
    }

    // 5. Globals (§ 6.4).
    let mut global_ids = HashMap::new();
    for global in &globals {
        let mut cx = FnCx::new(&program, &names, &func_names, &global_ids, usize::MAX, None);
        let init = cx.expr(&global.init, Some(global.ty))?;
        let hir::ExprKind::Const(init) = init.kind else {
            unreachable!("the grammar only allows literals")
        };
        global_ids.insert(global.name.name.as_str(), program.globals.len());
        program.globals.push(hir::GlobalDef {
            name: global.name.name.clone(),
            ty: global.ty,
            mutable: global.mutable,
            init,
        });
    }

    // 6. Function bodies.
    for (id, func) in funcs.iter().enumerate() {
        let iface = program.funcs[id].interface;
        let mut cx = FnCx::new(&program, &names, &func_names, &global_ids, iface, func.ret);
        cx.scopes.push(Vec::new());
        for param in &func.params {
            cx.declare(&param.name, param.ty)?;
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

    Ok(program)
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

fn mismatch<T>(span: Span, expected: Type, found: Type) -> Result<T> {
    err(
        Code::TypeMismatch,
        span,
        format!("mismatched types: expected `{expected}`, found `{found}`"),
    )
}

/// Analysis context of a function (or of the initial value of a global).
struct FnCx<'a> {
    program: &'a hir::Program,
    names: &'a HashMap<&'a str, TopLevel>,
    func_names: &'a [HashMap<&'a str, FuncId>],
    globals: &'a HashMap<&'a str, hir::GlobalId>,
    iface: InterfaceId,
    ret: Option<Type>,
    locals: Vec<Type>,
    /// Nested scopes: visible names and their variable.
    scopes: Vec<Vec<(String, LocalId)>>,
    loop_depth: u32,
}

impl<'a> FnCx<'a> {
    fn new(
        program: &'a hir::Program,
        names: &'a HashMap<&'a str, TopLevel>,
        func_names: &'a [HashMap<&'a str, FuncId>],
        globals: &'a HashMap<&'a str, hir::GlobalId>,
        iface: InterfaceId,
        ret: Option<Type>,
    ) -> Self {
        FnCx {
            program,
            names,
            func_names,
            globals,
            iface,
            ret,
            locals: Vec::new(),
            scopes: Vec::new(),
            loop_depth: 0,
        }
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
        if matches!(self.names.get(name.name.as_str()), Some(TopLevel::Global)) {
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
                // The initial value does not see the local yet (§ 9.4).
                let init = match init {
                    Some(e) => Some(self.expr(e, Some(*ty))?),
                    None => None,
                };
                let local = self.declare(name, *ty)?;
                hir::Stmt::Local { local, init }
            }
            StmtKind::Assign { target, value } => {
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
                                global.name, global.name, global.ty
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
                        format!("`return` must return a value of type `{ty}`"),
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
        if ty != Type::Bool {
            return Err(non_bool(&format!("`{ty}`")));
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
            ExprKind::Bool(b) => typed(hir::ExprKind::Const(Const::bool(*b)), Type::Bool),
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
            ExprKind::Convert { ty, arg } => self.convert(*ty, arg)?,
            ExprKind::Unary { op, expr } => return self.unary(*op, expr, e.span, expected),
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, e.span, expected)?,
        };
        let ty = result.value_ty();
        match expected {
            Some(exp) if exp != ty => mismatch(e.span, exp, ty),
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
                        "integer literal used for the float type `{ty}`: write for example `{}.0`",
                        if negative {
                            format!("-{value}")
                        } else {
                            value.to_string()
                        }
                    ),
                );
            }
            Some(Type::Bool) => {
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
                format!("`-` cannot be applied to the unsigned type `{ty}`"),
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
                format!("literal out of range for type `{ty}` ({min} … {max})"),
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
                    format!("float literal used for type `{ty}`"),
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
                format!("literal out of range for type `{ty}`"),
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
                        format!("`-` cannot be applied to type `{ty}`"),
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
                if ty != Type::Bool {
                    return err(
                        Code::InvalidOperator,
                        span,
                        format!("`!` only applies to type `bool`, found `{ty}`"),
                    );
                }
                if let Some(exp) = expected
                    && exp != Type::Bool
                {
                    return mismatch(span, exp, Type::Bool);
                }
                Ok(typed(
                    hir::ExprKind::Unary {
                        op,
                        expr: Box::new(inner),
                    },
                    Type::Bool,
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
                Type::Bool,
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
                    "operator `{}` cannot be applied to type `{ty}`{hint}",
                    op.symbol()
                ),
            );
        }
        let result_ty = if op.is_comparison() { Type::Bool } else { ty };
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
        if ty != Type::Bool {
            return err(
                Code::InvalidOperator,
                e.span,
                format!(
                    "operator `{}` only applies to type `bool`, found `{ty}`",
                    op.symbol()
                ),
            );
        }
        Ok(operand)
    }

    fn convert(&mut self, target: Type, arg: &ast::Expr) -> Result<hir::Expr> {
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
                    "cannot convert `{source}` to `{target}`: only conversions between numeric types exist"
                ),
            );
        }
        Ok(typed(hir::ExprKind::Convert(Box::new(inner)), target))
    }

    /// Analyzes a call. The result has `ty: None` if the function returns nothing.
    fn call(&mut self, call: &ast::Call) -> Result<hir::Expr> {
        let iface = match &call.iface {
            None => self.iface,
            Some(name) => match self.names.get(name.name.as_str()) {
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
        let Some(&func_id) = self.func_names[iface].get(call.name.name.as_str()) else {
            let mut message = format!(
                "interface `{iface_name}` has no function `{}`",
                call.name.name
            );
            if call.iface.is_none()
                && let Some(other) = self
                    .func_names
                    .iter()
                    .position(|f| f.contains_key(call.name.name.as_str()))
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
