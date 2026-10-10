//! Syntax tree produced by the parser.

use crate::source::Span;
use crate::types::Type;

#[derive(Debug, Clone)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Debug)]
pub enum Item {
    World(World),
    Interface(Interface),
    Global(Global),
}

impl Item {
    pub fn name(&self) -> &Ident {
        match self {
            Item::World(w) => &w.name,
            Item::Interface(i) => &i.name,
            Item::Global(g) => &g.name,
        }
    }
}

#[derive(Debug)]
pub struct World {
    pub name: Ident,
    pub exports: Vec<Ident>,
}

#[derive(Debug)]
pub struct Interface {
    pub name: Ident,
    pub funcs: Vec<Func>,
}

#[derive(Debug)]
pub struct Global {
    pub name: Ident,
    pub mutable: bool,
    pub ty: Type,
    /// Initial value: a literal, optionally preceded by `-`.
    pub init: Expr,
}

#[derive(Debug)]
pub struct Func {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    pub body: Block,
}

#[derive(Debug)]
pub struct Param {
    pub name: Ident,
    pub ty: Type,
}

#[derive(Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum StmtKind {
    Local {
        name: Ident,
        ty: Type,
        init: Option<Expr>,
    },
    Assign {
        target: Ident,
        value: Expr,
    },
    If(If),
    While {
        cond: Expr,
        body: Block,
    },
    Break,
    Continue,
    Return(Option<Expr>),
    Call(Call),
}

#[derive(Debug)]
pub struct If {
    pub cond: Expr,
    pub then: Block,
    pub else_: Option<Else>,
}

#[derive(Debug)]
pub enum Else {
    If(Box<If>),
    Block(Block),
}

#[derive(Debug)]
pub struct Call {
    /// Interface named by an `interface::function` path.
    pub iface: Option<Ident>,
    pub name: Ident,
    pub args: Vec<Expr>,
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum ExprKind {
    Int(u128),
    Float(String),
    Bool(bool),
    Name(String),
    Call(Call),
    Convert {
        ty: Type,
        arg: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Paren(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

impl UnaryOp {
    pub fn symbol(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }

    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        )
    }
}
