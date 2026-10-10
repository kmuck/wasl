//! Typed intermediate representation (HIR).
//!
//! The HIR is produced by semantic analysis: every name is resolved, every expression is
//! typed and every language rule is checked. Code generation has nothing left to verify.

use crate::ast::{BinOp, UnaryOp};
use crate::types::Type;

pub type GlobalId = usize;
pub type FuncId = usize;
pub type InterfaceId = usize;
/// Index of a variable in [`FuncDef::locals`] (parameters included).
pub type LocalId = u32;

#[derive(Debug, Default)]
pub struct Program {
    pub globals: Vec<GlobalDef>,
    pub funcs: Vec<FuncDef>,
    pub interfaces: Vec<InterfaceDef>,
    pub worlds: Vec<WorldDef>,
}

impl Program {
    /// Whether an interface is exported by at least one world.
    pub fn is_exported(&self, iface: InterfaceId) -> bool {
        self.worlds.iter().any(|w| w.exports.contains(&iface))
    }
}

#[derive(Debug)]
pub struct GlobalDef {
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
    pub init: Const,
}

#[derive(Debug)]
pub struct InterfaceDef {
    pub name: String,
    pub funcs: Vec<FuncId>,
}

#[derive(Debug)]
pub struct WorldDef {
    pub name: String,
    pub exports: Vec<InterfaceId>,
}

#[derive(Debug)]
pub struct FuncDef {
    pub name: String,
    pub interface: InterfaceId,
    pub params: Vec<(String, Type)>,
    pub ret: Option<Type>,
    /// Types of all the variables of the function: parameters first, then locals.
    pub locals: Vec<Type>,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub enum Stmt {
    /// Local declaration; without an initial value, the local is zero.
    Local {
        local: LocalId,
        init: Option<Expr>,
    },
    Assign {
        place: Place,
        value: Expr,
    },
    If {
        cond: Expr,
        then: Vec<Stmt>,
        else_: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Break,
    Continue,
    Return(Option<Expr>),
    /// Function call used as a statement; its result, if any, is discarded.
    Call(Expr),
}

#[derive(Debug, Clone, Copy)]
pub enum Place {
    Local(LocalId),
    Global(GlobalId),
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    /// `None` only for a call to a function without a return value.
    pub ty: Option<Type>,
}

impl Expr {
    /// Type of an expression used as a value (always present, except for void calls).
    pub fn value_ty(&self) -> Type {
        self.ty.expect("expression without a value")
    }
}

#[derive(Debug)]
pub enum ExprKind {
    Const(Const),
    Local(LocalId),
    Global(GlobalId),
    Call {
        func: FuncId,
        args: Vec<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    /// Both operands have the same type.
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// Conversion from the type of `expr` to the type of the expression.
    Convert(Box<Expr>),
}

/// Constant in its Wasm representation (8- and 16-bit integers normalized, § 12.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Const {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl Const {
    /// Builds a constant of an integer type from any value, reduced modulo 2ⁿ
    /// (wrapping).
    pub fn int(ty: Type, value: i128) -> Const {
        let bits = ty.bits();
        let modulus = 1i128 << bits;
        let mut v = value.rem_euclid(modulus);
        if ty.is_signed() && v >= modulus / 2 {
            v -= modulus;
        }
        if bits <= 32 {
            // 32-bit unsigned integers are stored with the same bits in an i32.
            Const::I32(v as i64 as i32)
        } else {
            Const::I64(v as i64)
        }
    }

    pub fn bool(value: bool) -> Const {
        Const::I32(value as i32)
    }
}
