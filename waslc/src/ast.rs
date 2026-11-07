//! Syntax tree produced by the parser.

use crate::source::Span;
use crate::types::Prim;

#[derive(Debug, Clone)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Debug)]
pub enum Item {
    World(World),
    Interface(Interface),
    Global(Box<Global>),
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
    /// `import` and `export` items, in source order.
    pub items: Vec<WorldItem>,
}

#[derive(Debug)]
pub struct WorldItem {
    pub import: bool,
    pub iface: InterfaceRef,
    /// The implementation of an exported interface of another package, for this world
    /// only (§ 2.4.2): its functions, as an interface named like the foreign one.
    pub body: Option<Interface>,
}

/// The name of an interface in a world (§ 6.2): an interface of the package, or an
/// interface of another package, `namespace:package/interface` (§ 2.5).
#[derive(Debug, Clone)]
pub enum InterfaceRef {
    Local(Ident),
    Foreign(ForeignName),
}

impl InterfaceRef {
    pub fn span(&self) -> Span {
        match self {
            InterfaceRef::Local(name) => name.span,
            InterfaceRef::Foreign(f) => f.namespace.span.to(f.name.span),
        }
    }
}

/// `namespace:package/interface`, with WaSL identifiers.
#[derive(Debug, Clone)]
pub struct ForeignName {
    pub namespace: Ident,
    pub package: Ident,
    pub name: Ident,
}

impl ForeignName {
    /// The name as written in WaSL: `wasi:cli/stdout`.
    pub fn display(&self) -> String {
        format!(
            "{}:{}/{}",
            self.namespace.name, self.package.name, self.name.name
        )
    }
}

#[derive(Debug)]
pub struct Interface {
    pub name: Ident,
    /// Functions and type declarations, in declaration order.
    pub items: Vec<InterfaceItem>,
}

impl Interface {
    pub fn funcs(&self) -> impl Iterator<Item = &Func> {
        self.items.iter().filter_map(|item| match item {
            InterfaceItem::Func(f) => Some(f),
            InterfaceItem::Type(_) | InterfaceItem::Resource(_) => None,
        })
    }

    pub fn types(&self) -> impl Iterator<Item = &TypeDecl> {
        self.items.iter().filter_map(|item| match item {
            InterfaceItem::Type(t) => Some(t),
            InterfaceItem::Func(_) | InterfaceItem::Resource(_) => None,
        })
    }
}

#[derive(Debug)]
pub enum InterfaceItem {
    Func(Func),
    Type(TypeDecl),
    Resource(ResourceDecl),
}

/// Declaration of a `resource` (§ 7.12): its private fields, then its constructor,
/// methods and static functions, in declaration order.
#[derive(Debug)]
pub struct ResourceDecl {
    pub name: Ident,
    pub fields: Vec<Field>,
    pub funcs: Vec<ResourceFunc>,
}

#[derive(Debug)]
pub struct ResourceFunc {
    pub kind: ResourceFuncKind,
    /// For the constructor, the name is `constructor` and there is no return type.
    pub func: Func,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceFuncKind {
    Constructor,
    Method,
    Static,
}

/// Declaration of a `record`, `enum`, `variant` or `flags` type (§ 7.7).
#[derive(Debug)]
pub struct TypeDecl {
    pub name: Ident,
    pub kind: TypeDeclKind,
}

#[derive(Debug)]
pub enum TypeDeclKind {
    Record(Vec<Field>),
    Enum(Vec<Ident>),
    Variant(Vec<Case>),
    Flags(Vec<Ident>),
}

#[derive(Debug)]
pub struct Field {
    pub name: Ident,
    pub mutable: bool,
    pub ty: TypeExpr,
}

#[derive(Debug)]
pub struct Case {
    pub name: Ident,
    pub payload: Option<TypeExpr>,
}

/// A type as written in the source (§ 5.2).
#[derive(Debug, Clone)]
pub struct TypeExpr {
    pub kind: TypeExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeExprKind {
    Prim(Prim),
    Char,
    String,
    List {
        mutable: bool,
        elem: Box<TypeExpr>,
    },
    Tuple(Vec<TypeExpr>),
    Option(Box<TypeExpr>),
    /// `result`, `result<T>`, `result<_, E>` or `result<T, E>`.
    Result {
        ok: Option<Box<TypeExpr>>,
        err: Option<Box<TypeExpr>>,
    },
    /// A declared type: `line` or `invoices::line`.
    Named(TypeName),
    /// `borrow<cart>` (§ 7.12).
    Borrow(TypeName),
}

/// Name of a declared type, with the interface that declares it if written.
#[derive(Debug, Clone)]
pub struct TypeName {
    pub iface: Option<Ident>,
    pub name: Ident,
}

impl TypeName {
    pub fn span(&self) -> Span {
        match &self.iface {
            Some(iface) => iface.span.to(self.name.span),
            None => self.name.span,
        }
    }
}

#[derive(Debug)]
pub struct Global {
    pub name: Ident,
    pub mutable: bool,
    pub ty: TypeExpr,
    pub init: Expr,
}

#[derive(Debug)]
pub struct Func {
    /// `async func` (§ 2.8).
    pub is_async: bool,
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub body: Block,
}

#[derive(Debug)]
pub struct Param {
    pub name: Ident,
    pub ty: TypeExpr,
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
        ty: TypeExpr,
        init: Option<Expr>,
    },
    /// `local (a: A, _) = e;` (§ 9.4): one local per element of a tuple, or `None` for
    /// an element written `_`.
    Destructure {
        bindings: Vec<Option<(Ident, TypeExpr)>>,
        init: Expr,
    },
    Assign {
        target: Place,
        value: Expr,
    },
    If(If),
    While {
        cond: Expr,
        body: Block,
    },
    Match(Match),
    Break,
    Continue,
    Return(Option<Expr>),
    /// A call statement (§ 5.3): a call, possibly followed by `?`.
    Call(Expr),
    /// `_ = f(…);`: a call statement whose value is discarded (§ 9.6).
    Discard(Expr),
}

/// Target of an assignment: a name followed by field accesses and indexes (§ 5.3).
#[derive(Debug)]
pub struct Place {
    pub name: Ident,
    pub selectors: Vec<Selector>,
}

#[derive(Debug)]
pub enum Selector {
    /// `.name`
    Field(Ident),
    /// `.0`: element of a tuple.
    Elem(u128, Span),
    /// `[index]`
    Index(Expr),
}

#[derive(Debug)]
pub struct Match {
    pub scrutinee: Expr,
    pub cases: Vec<MatchCase>,
    pub else_: Option<Block>,
}

#[derive(Debug)]
pub struct MatchCase {
    pub name: Ident,
    pub binding: Option<Ident>,
    pub body: Block,
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

/// A call `name(…)`, `a::name(…)` or `a::b::name(…)`: a function, a built-in function or
/// a constructor with a payload, told apart by semantic analysis (§ 9.8).
#[derive(Debug)]
pub struct Call {
    /// The names before the last `::`: none, one or two.
    pub path: Vec<Ident>,
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
    Str(String),
    Char(char),
    Name(String),
    /// `a::b` or `a::b::c`: a case, or a constructor without payload (§ 9.8).
    Path(Vec<Ident>),
    Call(Call),
    /// `T(x)`, where `T` is a primitive type, `char` or `string`.
    Convert {
        ty: TypeExpr,
        arg: Box<Expr>,
    },
    /// `r { f: v, … }`
    Record {
        ty: TypeName,
        fields: Vec<(Ident, Expr)>,
    },
    /// `f { a, b }`
    Flags {
        ty: TypeName,
        flags: Vec<Ident>,
    },
    /// `[a, b]`
    List(Vec<Expr>),
    /// `tuple(a, b)`
    Tuple(Vec<Expr>),
    /// `e.f`
    Field {
        expr: Box<Expr>,
        name: Ident,
    },
    /// `e.0`
    Elem {
        expr: Box<Expr>,
        index: u128,
        index_span: Span,
    },
    /// `e[i]`
    Index {
        expr: Box<Expr>,
        index: Box<Expr>,
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
    /// `e?` (§ 8.19)
    Try(Box<Expr>),
    /// `e.m(…)`: a method call (§ 8.20).
    Method {
        receiver: Box<Expr>,
        name: Ident,
        args: Vec<Expr>,
    },
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
