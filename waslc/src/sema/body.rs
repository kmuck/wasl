//! Analysis of function bodies and of the initial values of globals: names, typing and
//! control flow (§ 8 to 10, chapter v2.4).

use std::collections::HashMap;

use super::{Resolver, TopLevel};
use crate::ast::{self, BinOp, ExprKind, Ident, StmtKind, UnaryOp};
use crate::diag::{Code, Diagnostic, Result, err};
use crate::hir::{self, Const, FuncId, InterfaceId, LocalId, Place};
use crate::source::Span;
use crate::types::{DeclId, DeclKind, Prim, Type, TypeDecl, TypeKind, Types};

/// A block always returns if one of its statements always returns (§ 8.8, § v2.4.15).
pub(super) fn always_returns(stmts: &[hir::Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        hir::Stmt::Return(_) => true,
        hir::Stmt::If { then, else_, .. } => always_returns(then) && always_returns(else_),
        hir::Stmt::Match { arms, else_, .. } => {
            arms.iter().all(|a| always_returns(&a.body))
                && else_.as_ref().is_none_or(|e| always_returns(e))
        }
        _ => false,
    })
}

/// Whether an expression determines its own type (§ v2.4.1). Numeric literals,
/// `option::none`, `result::…`, `[]` and lists of literals take the expected type.
fn self_typed(e: &ast::Expr) -> bool {
    if needs_type_context(e) {
        return false;
    }
    match &e.kind {
        ExprKind::Paren(inner) => self_typed(inner),
        ExprKind::List(elems) => elems.iter().any(self_typed),
        ExprKind::Path(segments) => !matches!(
            (segments[0].name.as_str(), segments.len()),
            ("option" | "result", 2)
        ),
        ExprKind::Call(call) => match (call.path.as_slice(), call.name.name.as_str()) {
            ([head], "some") if head.name == "option" => call.args.iter().all(self_typed),
            ([head], _) if head.name == "result" => false,
            ([head], "filled") if head.name == "list" => call.args.get(1).is_some_and(self_typed),
            _ => true,
        },
        _ => true,
    }
}

/// What a path designates (§ v2.4.2).
enum PathTarget {
    Builtin(hir::Builtin),
    /// `option::c` or `result::c`, with `c` not yet checked.
    Option(Ident),
    Result(Ident),
    /// A case of a declared enum or variant.
    Case {
        decl: DeclId,
        case: u32,
    },
    /// A function of an interface.
    Func(FuncId),
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
pub(super) struct FnCx<'a> {
    program: &'a hir::Program,
    types: &'a mut Types,
    resolver: &'a Resolver<'a>,
    globals: &'a HashMap<&'a str, hir::GlobalId>,
    /// Interface of the function; `None` for the initial value of a global.
    iface: Option<InterfaceId>,
    ret: Option<Type>,
    pub(super) locals: Vec<Type>,
    /// Nested scopes: visible names and their variable.
    pub(super) scopes: Vec<Vec<(String, LocalId)>>,
    loop_depth: u32,
}

impl<'a> FnCx<'a> {
    pub(super) fn new(
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

    // ---- Variables ----

    fn lookup_local(&self, name: &str) -> Option<LocalId> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|s| s.iter().rev())
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }

    pub(super) fn declare(&mut self, name: &Ident, ty: Type) -> Result<LocalId> {
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

    pub(super) fn block(&mut self, block: &ast::Block) -> Result<Vec<hir::Stmt>> {
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
            StmtKind::Local {
                name,
                ty: written,
                init,
            } => {
                let ty = self.resolver.resolve(self.types, written, self.iface)?;
                let is_enum = matches!(
                    self.types.decl(ty),
                    Some((
                        _,
                        TypeDecl {
                            kind: DeclKind::Enum(_),
                            ..
                        }
                    ))
                );
                if init.is_none() && (!self.types.is_scalar(ty) || is_enum) {
                    return err(
                        Code::Uninitialized,
                        stmt.span,
                        format!(
                            "local `{}` of type `{}` needs an initial value: {}",
                            name.name,
                            self.show(ty),
                            if is_enum {
                                "no case of an enum is a default value"
                            } else {
                                "there is no default value for this type"
                            }
                        ),
                    );
                }
                // The initial value does not see the local yet (§ 9.4).
                let init = match init {
                    Some(e) => Some(self.expr(e, Some(ty))?),
                    None => None,
                };
                let local = self.declare(name, ty)?;
                hir::Stmt::Local { local, init }
            }
            StmtKind::Assign { target, value } if !target.selectors.is_empty() => {
                let (place, ty) = self.place(target)?;
                let value = self.expr(value, Some(ty))?;
                hir::Stmt::Assign { place, value }
            }
            StmtKind::Assign { target, value } => {
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
            StmtKind::Match(m) => self.match_(m, stmt.span)?,
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
            StmtKind::Call(call) => {
                let target = self.call_target(call)?;
                if !matches!(target, PathTarget::Func(_) | PathTarget::Builtin(_)) {
                    return err(
                        Code::Syntax,
                        stmt.span,
                        "only function calls can be used as statements, not constructors",
                    );
                }
                hir::Stmt::Call(self.call(call, target, stmt.span, None)?)
            }
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
    pub(super) fn expr(&mut self, e: &ast::Expr, expected: Option<Type>) -> Result<hir::Expr> {
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
                let target = self.call_target(call)?;
                if !matches!(target, PathTarget::Func(_)) {
                    return self.call(call, target, e.span, expected);
                }
                let result = self.call(call, target, e.span, expected)?;
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
            ExprKind::Str(text) => typed(hir::ExprKind::Str(text.clone()), Type::STRING),
            ExprKind::Char(c) => typed(hir::ExprKind::Const(Const::I32(*c as i32)), Type::CHAR),
            ExprKind::Path(segments) => {
                let target = self.path(segments)?;
                return self.construct(target, segments.last().unwrap(), None, e.span, expected);
            }
            ExprKind::Record { ty, fields } => self.record(ty, fields, e.span)?,
            ExprKind::Flags { ty, flags } => self.flags(ty, flags)?,
            ExprKind::List(elems) => return self.list(elems, e.span, expected),
            ExprKind::Tuple(elems) => return self.tuple(elems, e.span, expected),
            ExprKind::Field { expr, name } => self.field(expr, name)?,
            ExprKind::Elem {
                expr,
                index,
                index_span,
            } => self.elem(expr, *index, *index_span)?,
            ExprKind::Index { expr, index } => self.index(expr, index)?,
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
        let (l, r) = if self_typed(lhs) {
            let l = self.expr(lhs, None)?;
            let r = self.expr(rhs, Some(l.value_ty()))?;
            (l, r)
        } else if self_typed(rhs) {
            let r = self.expr(rhs, None)?;
            let l = self.expr(lhs, Some(r.value_ty()))?;
            (l, r)
        } else {
            match expected.filter(|_| !op.is_comparison()) {
                Some(ty) => (self.expr(lhs, Some(ty))?, self.expr(rhs, Some(ty))?),
                None if needs_type_context(lhs) => return untyped_literal(lhs.span),
                None => return cannot_infer(lhs.span),
            }
        };

        let ty = l.value_ty();
        let (is_enum, is_flags) = match self.types.decl(ty) {
            Some((_, decl)) => (
                matches!(decl.kind, DeclKind::Enum(_)),
                matches!(decl.kind, DeclKind::Flags(_)),
            ),
            None => (false, false),
        };
        let allowed = match op {
            BinOp::Add => ty.is_numeric() || ty == Type::STRING,
            BinOp::Sub | BinOp::Mul | BinOp::Div => ty.is_numeric(),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => ty.is_numeric() || ty == Type::CHAR,
            BinOp::BitAnd | BinOp::BitOr => ty.is_int() || is_flags,
            BinOp::Rem | BinOp::BitXor | BinOp::Shl | BinOp::Shr => ty.is_int(),
            BinOp::Eq | BinOp::Ne => {
                ty.as_prim().is_some()
                    || ty == Type::CHAR
                    || ty == Type::STRING
                    || is_enum
                    || is_flags
            }
            BinOp::And | BinOp::Or => unreachable!(),
        };
        if !allowed {
            let hint = if op == BinOp::Rem && ty.is_float() {
                " (Wasm has no float remainder)"
            } else if op.is_comparison() && ty == Type::STRING {
                " (strings have no order: ordering them is the job of a library)"
            } else if matches!(op, BinOp::Eq | BinOp::Ne) {
                " (compare their fields, or use `match`)"
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
        if source == Type::CHAR {
            if target != Prim::U32 {
                return err(
                    Code::InvalidConversion,
                    arg.span,
                    format!(
                        "cannot convert `char` to `{target}`: a `char` only converts to `u32`, its code point"
                    ),
                );
            }
            return Ok(typed(hir::ExprKind::Convert(Box::new(inner)), Type::U32));
        }
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

    // ---- Paths and calls (§ v2.4.2) ----

    /// What the function name of a call designates.
    fn call_target(&self, call: &ast::Call) -> Result<PathTarget> {
        if !call.path.is_empty() {
            return self.resolve_path(&call.path, &call.name);
        }
        let iface = self.iface.expect("calls are in functions");
        let scopes = self.resolver.scopes;
        if let Some(&id) = scopes[iface].funcs.get(call.name.name.as_str()) {
            return Ok(PathTarget::Func(id));
        }
        let iface_name = &self.program.interfaces[iface].name;
        let mut message = format!(
            "interface `{iface_name}` has no function `{}`",
            call.name.name
        );
        if let Some(other) = scopes
            .iter()
            .position(|s| s.funcs.contains_key(call.name.name.as_str()))
        {
            message += &format!(
                "; to call the one from another interface, write `{}::{}(…)`",
                self.program.interfaces[other].name, call.name.name
            );
        }
        err(Code::UnknownFunc, call.name.span, message)
    }

    /// What a path used as a value designates: a case or a constructor without payload.
    fn path(&self, segments: &[Ident]) -> Result<PathTarget> {
        let (name, path) = segments.split_last().expect("a path has a name");
        let target = self.resolve_path(path, name)?;
        if matches!(target, PathTarget::Builtin(_) | PathTarget::Func(_)) {
            let written: Vec<&str> = segments.iter().map(|s| s.name.as_str()).collect();
            return err(
                Code::UnknownName,
                segments[0].span.to(name.span),
                format!(
                    "`{}` is a function: functions are not values, call it with `{}(…)`",
                    written.join("::"),
                    written.join("::")
                ),
            );
        }
        Ok(target)
    }

    /// Resolves `path::name`, where `path` has one or two names (§ v2.4.2).
    fn resolve_path(&self, path: &[Ident], name: &Ident) -> Result<PathTarget> {
        let head = &path[0];
        let scopes = self.resolver.scopes;
        if path.len() == 1 {
            match head.name.as_str() {
                "string" | "list" | "char" => {
                    return match hir::Builtin::from_path(&head.name, &name.name) {
                        Some(b) => Ok(PathTarget::Builtin(b)),
                        None => err(
                            Code::UnknownFunc,
                            name.span,
                            format!("`{}` has no built-in function `{}`", head.name, name.name),
                        ),
                    };
                }
                "option" => return Ok(PathTarget::Option(name.clone())),
                "result" => return Ok(PathTarget::Result(name.clone())),
                _ => {}
            }
            if let Some(iface) = self.iface
                && let Some(&decl) = scopes[iface].types.get(head.name.as_str())
            {
                return self.case_of(decl, head, name);
            }
        }
        let iface = match self.resolver.names.get(head.name.as_str()) {
            Some(TopLevel::Interface(id)) => *id,
            Some(other) => {
                return err(
                    Code::UnknownName,
                    head.span,
                    format!("`{}` is {}, not an interface", head.name, other.describe()),
                );
            }
            None => {
                return err(
                    Code::UnknownName,
                    head.span,
                    format!("unknown interface or type `{}`", head.name),
                );
            }
        };
        let scope = &scopes[iface];
        if let [_, ty] = path {
            return match scope.types.get(ty.name.as_str()) {
                Some(&decl) => self.case_of(decl, ty, name),
                None => err(
                    Code::UnknownType,
                    ty.span,
                    format!("interface `{}` has no type `{}`", head.name, ty.name),
                ),
            };
        }
        match scope.funcs.get(name.name.as_str()) {
            Some(&id) => Ok(PathTarget::Func(id)),
            None if scope.types.contains_key(name.name.as_str()) => err(
                Code::UnknownCase,
                name.span,
                format!(
                    "`{}::{}` is a type, not a value: write one of its cases, `{}::{}::…`",
                    head.name, name.name, head.name, name.name
                ),
            ),
            None => err(
                Code::UnknownFunc,
                name.span,
                format!("interface `{}` has no function `{}`", head.name, name.name),
            ),
        }
    }

    /// The case `name` of the declared type `decl`, written `ty::name`.
    fn case_of(&self, decl: DeclId, ty: &Ident, name: &Ident) -> Result<PathTarget> {
        let def = &self.types.decls[decl];
        let cases: Vec<&str> = match &def.kind {
            DeclKind::Enum(cases) => cases.iter().map(String::as_str).collect(),
            DeclKind::Variant(cases) => cases.iter().map(|c| c.name.as_str()).collect(),
            other => {
                return err(
                    Code::UnknownCase,
                    name.span,
                    format!("`{}` is a {}, which has no cases", ty.name, other.keyword()),
                );
            }
        };
        match cases.iter().position(|c| *c == name.name) {
            Some(case) => Ok(PathTarget::Case {
                decl,
                case: case as u32,
            }),
            None => err(
                Code::UnknownCase,
                name.span,
                format!("`{}` has no case `{}`", ty.name, name.name),
            ),
        }
    }

    /// Analyzes a call to `target`. The result has `ty: None` for a function without
    /// a return value.
    fn call(
        &mut self,
        call: &ast::Call,
        target: PathTarget,
        span: Span,
        expected: Option<Type>,
    ) -> Result<hir::Expr> {
        let func_id = match target {
            PathTarget::Func(id) => id,
            PathTarget::Builtin(b) => return self.builtin(b, call, span, expected),
            other => return self.construct(other, &call.name, Some(&call.args), span, expected),
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

    /// A case or a constructor, with its arguments if written with parentheses
    /// (§ v2.4.7, § v2.4.8).
    fn construct(
        &mut self,
        target: PathTarget,
        name: &Ident,
        args: Option<&[ast::Expr]>,
        span: Span,
        expected: Option<Type>,
    ) -> Result<hir::Expr> {
        match target {
            PathTarget::Case { decl, case } => {
                let ty = self.types.decl_type_of(decl);
                let payload = match &self.types.decls[decl].kind {
                    DeclKind::Variant(cases) => cases[case as usize].payload,
                    _ => None,
                };
                let is_enum = matches!(self.types.decls[decl].kind, DeclKind::Enum(_));
                let payload = self.payload(name, payload, args)?;
                let kind = if is_enum {
                    hir::ExprKind::Const(Const::I32(case as i32))
                } else {
                    hir::ExprKind::Case {
                        case,
                        payload: payload.map(Box::new),
                    }
                };
                self.expect_type(typed(kind, ty), expected, span)
            }
            PathTarget::Option(case) => {
                let (inner, opt) = match expected.map(|t| (t, self.types.kind(t).clone())) {
                    Some((t, TypeKind::Option(inner))) => (Some(inner), Some(t)),
                    Some((t, _)) => {
                        return self.mismatch_desc(span, t, "an option");
                    }
                    None => (None, None),
                };
                match case.name.as_str() {
                    "none" => {
                        self.payload(&case, None, args)?;
                        match opt {
                            Some(t) => Ok(typed(hir::ExprKind::None, t)),
                            None => cannot_infer(span),
                        }
                    }
                    "some" => {
                        if inner.is_none()
                            && args.is_some_and(|a| a.len() == 1 && !self_typed(&a[0]))
                        {
                            return cannot_infer(span);
                        }
                        let value = match inner {
                            Some(inner) => self.payload(&case, Some(inner), args)?,
                            None => {
                                let arg = self.single_arg(&case, args)?;
                                Some(self.expr(arg, None)?)
                            }
                        }
                        .expect("`some` has a payload");
                        let ty = match opt {
                            Some(t) => t,
                            None => self.types.intern(TypeKind::Option(value.value_ty())),
                        };
                        Ok(typed(hir::ExprKind::Some(Box::new(value)), ty))
                    }
                    other => err(
                        Code::UnknownCase,
                        case.span,
                        format!("`option` has no case `{other}`: its cases are `some` and `none`"),
                    ),
                }
            }
            PathTarget::Result(case) => {
                let index = match case.name.as_str() {
                    "ok" => hir::OK,
                    "err" => hir::ERR,
                    other => {
                        return err(
                            Code::UnknownCase,
                            case.span,
                            format!("`result` has no case `{other}`: its cases are `ok` and `err`"),
                        );
                    }
                };
                let (ty, ok, error) = match expected.map(|t| (t, self.types.kind(t).clone())) {
                    Some((t, TypeKind::Result { ok, err })) => (t, ok, err),
                    Some((t, _)) => return self.mismatch_desc(span, t, "a result"),
                    None => return cannot_infer(span),
                };
                let payload_ty = if index == hir::OK { ok } else { error };
                let payload = self.payload(&case, payload_ty, args)?;
                Ok(typed(
                    hir::ExprKind::Case {
                        case: index,
                        payload: payload.map(Box::new),
                    },
                    ty,
                ))
            }
            PathTarget::Builtin(_) | PathTarget::Func(_) => unreachable!("not a constructor"),
        }
    }

    /// The payload of a case written `name` or `name(args)`: E0407 when it is written
    /// for a case without payload, or missing for a case with one.
    fn payload(
        &mut self,
        name: &Ident,
        payload: Option<Type>,
        args: Option<&[ast::Expr]>,
    ) -> Result<Option<hir::Expr>> {
        match (payload, args) {
            (None, None) => Ok(None),
            (None, Some(_)) => err(
                Code::Payload,
                name.span,
                format!(
                    "case `{}` has no payload: write it without parentheses",
                    name.name
                ),
            ),
            (Some(ty), None) => err(
                Code::Payload,
                name.span,
                format!(
                    "case `{}` has a payload of type `{}`: write `{}(…)`",
                    name.name,
                    self.show(ty),
                    name.name
                ),
            ),
            (Some(ty), Some(_)) => {
                let arg = self.single_arg(name, args)?;
                Ok(Some(self.expr(arg, Some(ty))?))
            }
        }
    }

    fn single_arg<'e>(&self, name: &Ident, args: Option<&'e [ast::Expr]>) -> Result<&'e ast::Expr> {
        match args {
            Some([arg]) => Ok(arg),
            Some(args) => err(
                Code::ArgCount,
                name.span,
                format!(
                    "a payload is one value, {} given (use a record or a tuple for several)",
                    args.len()
                ),
            ),
            None => err(
                Code::Payload,
                name.span,
                format!(
                    "case `{}` needs a payload: write `{}(…)`",
                    name.name, name.name
                ),
            ),
        }
    }

    /// A call to a built-in function (§ v2.4.11).
    fn builtin(
        &mut self,
        func: hir::Builtin,
        call: &ast::Call,
        span: Span,
        expected: Option<Type>,
    ) -> Result<hir::Expr> {
        use hir::Builtin as B;
        let arity = if func == B::ListFilled { 2 } else { 1 };
        if call.args.len() != arity {
            return err(
                Code::ArgCount,
                call.name.span,
                format!(
                    "`{}::{}` expects {arity} argument(s), {} given",
                    call.path[0].name,
                    call.name.name,
                    call.args.len()
                ),
            );
        }
        let arg = &call.args[0];
        let list =
            |types: &mut Types, mutable, elem| types.intern(TypeKind::List { mutable, elem });
        let (args, ty) = match func {
            B::StringLen => (vec![self.expr(arg, Some(Type::STRING))?], Type::U32),
            B::StringBytes => (
                vec![self.expr(arg, Some(Type::STRING))?],
                list(self.types, false, Type::U8),
            ),
            B::StringChars => (
                vec![self.expr(arg, Some(Type::STRING))?],
                list(self.types, false, Type::CHAR),
            ),
            B::StringFromBytes => (
                vec![self.list_arg(arg, Some(Type::U8))?.0],
                self.types.intern(TypeKind::Option(Type::STRING)),
            ),
            B::StringFromChars => (vec![self.list_arg(arg, Some(Type::CHAR))?.0], Type::STRING),
            B::CharFromU32 => (
                vec![self.expr(arg, Some(Type::U32))?],
                self.types.intern(TypeKind::Option(Type::CHAR)),
            ),
            B::ListLen => (vec![self.list_arg(arg, None)?.0], Type::U32),
            B::ListCopy => {
                let (xs, elem) = self.list_arg(arg, None)?;
                (vec![xs], list(self.types, true, elem))
            }
            B::ListFreeze => {
                let xs = self.expr(arg, None)?;
                let ty = xs.value_ty();
                match self.types.kind(ty).clone() {
                    TypeKind::List {
                        mutable: true,
                        elem,
                    } => (vec![xs], list(self.types, false, elem)),
                    TypeKind::List { mutable: false, .. } => {
                        return err(
                            Code::TypeMismatch,
                            arg.span,
                            format!(
                                "`{}` is already immutable: `list::freeze` takes a `list<mut T>`",
                                self.show(ty)
                            ),
                        );
                    }
                    _ => {
                        return err(
                            Code::TypeMismatch,
                            arg.span,
                            format!(
                                "mismatched types: expected a `list<mut T>`, found `{}`",
                                self.show(ty)
                            ),
                        );
                    }
                }
            }
            B::ListFilled => {
                let n = self.expr(arg, Some(Type::U32))?;
                let value = &call.args[1];
                match expected.map(|t| (t, self.types.kind(t).clone())) {
                    Some((t, TypeKind::List { elem, .. })) => {
                        (vec![n, self.expr(value, Some(elem))?], t)
                    }
                    Some((t, _)) => return self.mismatch_desc(span, t, "a list"),
                    None => {
                        if !self_typed(value) {
                            return cannot_infer(value.span);
                        }
                        let x = self.expr(value, None)?;
                        let ty = list(self.types, false, x.value_ty());
                        (vec![n, x], ty)
                    }
                }
            }
        };
        self.expect_type(
            typed(hir::ExprKind::Builtin { func, args }, ty),
            expected,
            span,
        )
    }

    /// An argument of type `list<T>` or `list<mut T>`; `elem` is `T` when it is fixed.
    /// Returns the argument and its element type.
    fn list_arg(&mut self, arg: &ast::Expr, elem: Option<Type>) -> Result<(hir::Expr, Type)> {
        let e = match elem {
            Some(elem) if !self_typed(arg) => {
                let ty = self.types.intern(TypeKind::List {
                    mutable: false,
                    elem,
                });
                self.expr(arg, Some(ty))?
            }
            _ => self.expr(arg, None)?,
        };
        let ty = e.value_ty();
        match self.types.kind(ty) {
            TypeKind::List { elem: found, .. } if elem.is_none_or(|e| e == *found) => {
                let found = *found;
                Ok((e, found))
            }
            _ => {
                let expected = match elem {
                    Some(elem) => format!("`list<{}>`", self.show(elem)),
                    None => "a list".to_string(),
                };
                err(
                    Code::TypeMismatch,
                    arg.span,
                    format!(
                        "mismatched types: expected {expected}, found `{}`",
                        self.show(ty)
                    ),
                )
            }
        }
    }

    // ---- Literals (§ v2.4.4 to § v2.4.7) ----

    fn record(
        &mut self,
        name: &ast::TypeName,
        fields: &[(Ident, ast::Expr)],
        span: Span,
    ) -> Result<hir::Expr> {
        let decl = self.resolver.decl(name, self.iface)?;
        let defs: Vec<(String, Type)> = match &self.types.decls[decl].kind {
            DeclKind::Record(defs) => defs.iter().map(|f| (f.name.clone(), f.ty)).collect(),
            other => {
                return err(
                    Code::UnknownType,
                    name.span(),
                    format!(
                        "`{}` is a {}, not a record",
                        name.name.name,
                        other.keyword()
                    ),
                );
            }
        };
        for (i, (field, _)) in fields.iter().enumerate() {
            if defs.get(i).is_some_and(|(n, _)| *n == field.name) {
                continue;
            }
            let message = if !defs.iter().any(|(n, _)| *n == field.name) {
                return err(
                    Code::UnknownField,
                    field.span,
                    format!("record `{}` has no field `{}`", name.name.name, field.name),
                );
            } else if fields[..i].iter().any(|(f, _)| f.name == field.name) {
                format!("field `{}` is written twice", field.name)
            } else {
                format!(
                    "field `{}` must come here: the fields of a record literal are written in declaration order",
                    defs[i].0
                )
            };
            return err(Code::RecordFields, field.span, message);
        }
        if let Some((missing, _)) = defs.get(fields.len()) {
            return err(
                Code::RecordFields,
                span,
                format!(
                    "missing field `{missing}` in the literal of `{}`",
                    name.name.name
                ),
            );
        }
        let mut values = Vec::with_capacity(fields.len());
        for ((_, value), (_, ty)) in fields.iter().zip(&defs) {
            values.push(self.expr(value, Some(*ty))?);
        }
        let ty = self.types.decl_type_of(decl);
        Ok(typed(hir::ExprKind::Record(values), ty))
    }

    fn flags(&mut self, name: &ast::TypeName, flags: &[Ident]) -> Result<hir::Expr> {
        let decl = self.resolver.decl(name, self.iface)?;
        let defs: Vec<String> = match &self.types.decls[decl].kind {
            DeclKind::Flags(defs) => defs.clone(),
            other => {
                return err(
                    Code::UnknownType,
                    name.span(),
                    format!(
                        "`{}` is a {}, not a flags type",
                        name.name.name,
                        other.keyword()
                    ),
                );
            }
        };
        let mut bits = 0u32;
        let mut last = None;
        for flag in flags {
            let Some(index) = defs.iter().position(|d| *d == flag.name) else {
                return err(
                    Code::UnknownCase,
                    flag.span,
                    format!("`{}` has no flag `{}`", name.name.name, flag.name),
                );
            };
            if last.is_some_and(|l| index <= l) {
                return err(
                    Code::FlagsOrder,
                    flag.span,
                    format!(
                        "flag `{}` is repeated or out of order: the flags of a literal are written once, in declaration order",
                        flag.name
                    ),
                );
            }
            last = Some(index);
            bits |= 1 << index;
        }
        let ty = self.types.decl_type_of(decl);
        Ok(typed(hir::ExprKind::Const(Const::I32(bits as i32)), ty))
    }

    fn list(
        &mut self,
        elems: &[ast::Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Result<hir::Expr> {
        let (ty, elem) = match expected.map(|t| (t, self.types.kind(t).clone())) {
            Some((t, TypeKind::List { elem, .. })) => (t, elem),
            Some((t, _)) => return self.mismatch_desc(span, t, "a list"),
            None => {
                // The type of the first element that has one (§ v2.4.6).
                let Some(first) = elems.iter().position(self_typed) else {
                    return cannot_infer(span);
                };
                let value = self.expr(&elems[first], None)?;
                let elem = value.value_ty();
                let ty = self.types.intern(TypeKind::List {
                    mutable: false,
                    elem,
                });
                let mut values = Vec::with_capacity(elems.len());
                let mut first_value = Some(value);
                for (i, e) in elems.iter().enumerate() {
                    if i == first {
                        values.push(first_value.take().unwrap());
                    } else {
                        values.push(self.expr(e, Some(elem))?);
                    }
                }
                return Ok(typed(hir::ExprKind::List(values), ty));
            }
        };
        let values = elems
            .iter()
            .map(|e| self.expr(e, Some(elem)))
            .collect::<Result<_>>()?;
        Ok(typed(hir::ExprKind::List(values), ty))
    }

    fn tuple(
        &mut self,
        elems: &[ast::Expr],
        span: Span,
        expected: Option<Type>,
    ) -> Result<hir::Expr> {
        match expected.map(|t| (t, self.types.kind(t).clone())) {
            Some((t, TypeKind::Tuple(types))) => {
                if types.len() != elems.len() {
                    return err(
                        Code::TypeMismatch,
                        span,
                        format!(
                            "mismatched types: expected `{}`, found a tuple of {} element(s)",
                            self.show(t),
                            elems.len()
                        ),
                    );
                }
                let values = elems
                    .iter()
                    .zip(types)
                    .map(|(e, ty)| self.expr(e, Some(ty)))
                    .collect::<Result<_>>()?;
                Ok(typed(hir::ExprKind::Tuple(values), t))
            }
            Some((t, _)) => self.mismatch_desc(span, t, "a tuple"),
            None => {
                let mut values = Vec::with_capacity(elems.len());
                for e in elems {
                    if !self_typed(e) {
                        return cannot_infer(e.span);
                    }
                    values.push(self.expr(e, None)?);
                }
                let ty = self.types.intern(TypeKind::Tuple(
                    values.iter().map(|v| v.value_ty()).collect(),
                ));
                Ok(typed(hir::ExprKind::Tuple(values), ty))
            }
        }
    }

    // ---- Field accesses and indexes (§ v2.4.5, § v2.4.6) ----

    fn field(&mut self, expr: &ast::Expr, name: &Ident) -> Result<hir::Expr> {
        let base = self.expr(expr, None)?;
        self.field_of(base, name)
    }

    fn field_of(&mut self, base: hir::Expr, name: &Ident) -> Result<hir::Expr> {
        let ty = base.value_ty();
        let (kind, field_ty) = match self.types.decl(ty).map(|(_, d)| &d.kind) {
            Some(DeclKind::Record(fields)) => match fields.iter().position(|f| f.name == name.name)
            {
                Some(i) => (
                    hir::ExprKind::Field {
                        expr: Box::new(base),
                        field: i as u32,
                    },
                    fields[i].ty,
                ),
                None => {
                    return err(
                        Code::UnknownField,
                        name.span,
                        format!("record `{}` has no field `{}`", self.show(ty), name.name),
                    );
                }
            },
            Some(DeclKind::Flags(flags)) => match flags.iter().position(|f| *f == name.name) {
                Some(i) => (
                    hir::ExprKind::Flag {
                        expr: Box::new(base),
                        flag: i as u32,
                    },
                    Type::BOOL,
                ),
                None => {
                    return err(
                        Code::UnknownField,
                        name.span,
                        format!("flags type `{}` has no flag `{}`", self.show(ty), name.name),
                    );
                }
            },
            _ => {
                return err(
                    Code::UnknownField,
                    name.span,
                    format!(
                        "type `{}` has no field `{}`: only records and flags have named fields",
                        self.show(ty),
                        name.name
                    ),
                );
            }
        };
        Ok(typed(kind, field_ty))
    }

    fn elem(&mut self, expr: &ast::Expr, index: u128, span: Span) -> Result<hir::Expr> {
        let base = self.expr(expr, None)?;
        self.elem_of(base, index, span)
    }

    fn elem_of(&mut self, base: hir::Expr, index: u128, span: Span) -> Result<hir::Expr> {
        let ty = base.value_ty();
        match self.types.kind(ty) {
            TypeKind::Tuple(elems) if index < elems.len() as u128 => {
                let elem_ty = elems[index as usize];
                Ok(typed(
                    hir::ExprKind::Elem {
                        expr: Box::new(base),
                        index: index as u32,
                    },
                    elem_ty,
                ))
            }
            TypeKind::Tuple(elems) => err(
                Code::UnknownField,
                span,
                format!(
                    "`{}` has {} element(s), numbered from 0",
                    self.show(ty),
                    elems.len()
                ),
            ),
            _ => err(
                Code::UnknownField,
                span,
                format!(
                    "type `{}` has no element `{index}`: only tuples have numbered elements",
                    self.show(ty)
                ),
            ),
        }
    }

    fn index(&mut self, expr: &ast::Expr, index: &ast::Expr) -> Result<hir::Expr> {
        let base = self.expr(expr, None)?;
        self.index_of(base, expr.span, index)
    }

    fn index_of(&mut self, base: hir::Expr, span: Span, index: &ast::Expr) -> Result<hir::Expr> {
        let ty = base.value_ty();
        let elem = match self.types.kind(ty) {
            TypeKind::List { elem, .. } => *elem,
            _ => return self.not_indexable(span, ty),
        };
        let index = self.expr(index, Some(Type::U32))?;
        Ok(typed(
            hir::ExprKind::Index {
                expr: Box::new(base),
                index: Box::new(index),
            },
            elem,
        ))
    }

    fn not_indexable<T>(&self, span: Span, ty: Type) -> Result<T> {
        let message = if ty == Type::STRING {
            "strings cannot be indexed: their elements are bytes of an encoding; use `string::bytes(s)[i]` or `string::chars(s)[i]`".to_string()
        } else {
            format!("type `{}` cannot be indexed: only lists can", self.show(ty))
        };
        err(Code::InvalidOperator, span, message)
    }

    // ---- Assignment and `match` (§ v2.4.14, § v2.4.9) ----

    /// Target of an assignment with at least one selector: `inv.status`, `xs[i]`.
    fn place(&mut self, target: &ast::Place) -> Result<(Place, Type)> {
        let name = &target.name;
        let mut base = if let Some(id) = self.lookup_local(&name.name) {
            typed(hir::ExprKind::Local(id), self.locals[id as usize])
        } else if let Some(&id) = self.globals.get(name.name.as_str()) {
            typed(hir::ExprKind::Global(id), self.program.globals[id].ty)
        } else {
            return self.unknown_name(name);
        };
        let mut span = name.span;
        let (last, path) = target.selectors.split_last().expect("a selector");
        for selector in path {
            base = match selector {
                ast::Selector::Field(field) => {
                    span = span.to(field.span);
                    self.field_of(base, field)?
                }
                ast::Selector::Elem(index, index_span) => {
                    span = span.to(*index_span);
                    self.elem_of(base, *index, *index_span)?
                }
                ast::Selector::Index(index) => {
                    let b = self.index_of(base, span, index)?;
                    span = span.to(index.span);
                    b
                }
            };
        }
        let ty = base.value_ty();
        match last {
            ast::Selector::Field(field) => match self.types.decl(ty).map(|(_, d)| &d.kind) {
                Some(DeclKind::Record(fields)) => {
                    let Some(i) = fields.iter().position(|f| f.name == field.name) else {
                        return self.field_of(base, field).map(|_| unreachable!());
                    };
                    if !fields[i].mutable {
                        return err(
                            Code::AssignReadOnly,
                            field.span,
                            format!(
                                "field `{}` of `{}` is not `mut`: it is set once, when the record is created",
                                field.name,
                                self.show(ty)
                            ),
                        );
                    }
                    let field_ty = fields[i].ty;
                    Ok((
                        Place::Field {
                            record: base,
                            field: i as u32,
                        },
                        field_ty,
                    ))
                }
                Some(DeclKind::Flags(flags)) if flags.contains(&field.name) => err(
                    Code::AssignReadOnly,
                    field.span,
                    "a flag cannot be assigned: assign the whole value, e.g. `d = d | delivery { urgent };`",
                ),
                _ => self.field_of(base, field).map(|_| unreachable!()),
            },
            ast::Selector::Elem(index, index_span) => {
                self.elem_of(base, *index, *index_span)?;
                err(
                    Code::AssignReadOnly,
                    *index_span,
                    "the elements of a tuple cannot be assigned",
                )
            }
            ast::Selector::Index(index) => match self.types.kind(ty).clone() {
                TypeKind::List {
                    mutable: true,
                    elem,
                } => {
                    let index = self.expr(index, Some(Type::U32))?;
                    Ok((Place::Index { list: base, index }, elem))
                }
                TypeKind::List { elem, .. } => err(
                    Code::AssignReadOnly,
                    span,
                    format!(
                        "the elements of a `{}` never change: only a `list<mut {}>` can be assigned",
                        self.show(ty),
                        self.show(elem)
                    ),
                ),
                _ => self.not_indexable(span, ty),
            },
        }
    }

    fn match_(&mut self, m: &ast::Match, span: Span) -> Result<hir::Stmt> {
        let scrutinee = self.expr(&m.scrutinee, None)?;
        let ty = scrutinee.value_ty();
        let cases: Vec<(String, Option<Type>)> = match self.types.kind(ty).clone() {
            TypeKind::Option(inner) => vec![("none".into(), None), ("some".into(), Some(inner))],
            TypeKind::Result { ok, err } => vec![("ok".into(), ok), ("err".into(), err)],
            TypeKind::Decl(id) => match &self.types.decls[id].kind {
                DeclKind::Enum(cases) => cases.iter().map(|c| (c.clone(), None)).collect(),
                DeclKind::Variant(cases) => {
                    cases.iter().map(|c| (c.name.clone(), c.payload)).collect()
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        if cases.is_empty() {
            let hint = if ty.as_prim().is_some() || ty == Type::CHAR {
                ": use `if` to compare values"
            } else {
                ""
            };
            return err(
                Code::NotMatchable,
                m.scrutinee.span,
                format!(
                    "`match` applies to enums, variants, options and results, not to `{}`{hint}",
                    self.show(ty)
                ),
            );
        }
        let mut covered = vec![false; cases.len()];
        let mut arms = Vec::with_capacity(m.cases.len());
        for arm in &m.cases {
            let Some(index) = cases.iter().position(|(c, _)| *c == arm.name.name) else {
                return err(
                    Code::UnknownCase,
                    arm.name.span,
                    format!("`{}` has no case `{}`", self.show(ty), arm.name.name),
                );
            };
            if std::mem::replace(&mut covered[index], true) {
                return err(
                    Code::RedundantCase,
                    arm.name.span,
                    format!("case `{}` is already listed", arm.name.name),
                );
            }
            let payload = cases[index].1;
            self.scopes.push(Vec::new());
            let binding = match (&arm.binding, payload) {
                (Some(b), None) => {
                    return err(
                        Code::Payload,
                        b.span,
                        format!("case `{}` has no payload to name", arm.name.name),
                    );
                }
                (Some(b), Some(ty)) => Some(self.declare(b, ty)?),
                (None, _) => None,
            };
            let body = self.block(&arm.body)?;
            self.scopes.pop();
            arms.push(hir::MatchArm {
                case: index as u32,
                binding,
                body,
            });
        }
        let missing: Vec<String> = cases
            .iter()
            .zip(&covered)
            .filter(|(_, c)| !**c)
            .map(|((name, _), _)| format!("`{name}`"))
            .collect();
        let else_ = match &m.else_ {
            Some(_) if missing.is_empty() => {
                return err(
                    Code::RedundantCase,
                    span,
                    "this `else` covers no case: every case is already listed",
                );
            }
            Some(block) => Some(self.block(block)?),
            None if !missing.is_empty() => {
                return err(
                    Code::NonExhaustive,
                    span,
                    format!(
                        "`match` does not cover {}: list them, or end with an `else` block",
                        missing.join(", ")
                    ),
                );
            }
            None => None,
        };
        Ok(hir::Stmt::Match {
            scrutinee,
            arms,
            else_,
        })
    }

    // ---- Helpers ----

    /// Checks that `e` has the expected type, if any.
    fn expect_type(&self, e: hir::Expr, expected: Option<Type>, span: Span) -> Result<hir::Expr> {
        match expected {
            Some(exp) if exp != e.value_ty() => self.mismatch(span, exp, e.value_ty()),
            _ => Ok(e),
        }
    }

    /// E0400 for a value whose kind does not match the expected type.
    fn mismatch_desc<T>(&self, span: Span, expected: Type, found: &str) -> Result<T> {
        err(
            Code::TypeMismatch,
            span,
            format!(
                "mismatched types: expected `{}`, found {found}",
                self.show(expected)
            ),
        )
    }
}

fn typed(kind: hir::ExprKind, ty: Type) -> hir::Expr {
    hir::Expr { kind, ty: Some(ty) }
}

/// E0408: a value whose type comes from the context, without context.
fn cannot_infer<T>(span: Span) -> Result<T> {
    err(
        Code::CannotInfer,
        span,
        "cannot infer the type of this value: nothing in the context gives its type",
    )
}

fn untyped_literal<T>(span: Span) -> Result<T> {
    err(
        Code::UntypedLiteral,
        span,
        "cannot infer the type of the literal: nothing in the context gives its type",
    )
}
