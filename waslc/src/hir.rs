//! Typed intermediate representation (HIR).
//!
//! The HIR is produced by semantic analysis: every name is resolved, every expression is
//! typed and every language rule is checked. Code generation has nothing left to verify.

use crate::ast::{BinOp, UnaryOp};
use crate::types::{DeclId, Prim, Type, Types};

pub type GlobalId = usize;
pub type FuncId = usize;
pub type InterfaceId = usize;
/// Index of a variable in [`FuncDef::locals`] (parameters included).
pub type LocalId = u32;

#[derive(Debug, Default)]
pub struct Program {
    pub types: Types,
    pub globals: Vec<GlobalDef>,
    pub funcs: Vec<FuncDef>,
    pub interfaces: Vec<InterfaceDef>,
    pub worlds: Vec<WorldDef>,
    /// The handle types (§ 7.12), by declaration: the resources of the interfaces of
    /// other packages and of the interfaces of the package that a world imports.
    pub handles: Vec<bool>,
}

impl Program {
    /// Whether an interface is exported by at least one world.
    pub fn is_exported(&self, iface: InterfaceId) -> bool {
        self.worlds.iter().any(|w| w.exports.contains(&iface))
    }

    /// Whether an interface of the package is in the WIT with its functions: a world
    /// exports or imports it (§ 2.5, § 13.1).
    pub fn has_wit_funcs(&self, iface: InterfaceId) -> bool {
        self.interfaces[iface].foreign.is_none()
            && self.interfaces[iface].implements.is_none()
            && self
                .worlds
                .iter()
                .any(|w| w.exports.contains(&iface) || w.imports.contains(&iface))
    }

    /// The functions compiled into the component of `world`: those reachable from its
    /// exports, without entering the functions of the interfaces it imports and of the
    /// interfaces of other packages (§ 2.3, § 2.5). Also returns the functions of other
    /// components that they call, in order of their id.
    pub fn reach(&self, world: usize) -> (Vec<bool>, Vec<FuncId>) {
        let world = &self.worlds[world];
        let mut used = vec![false; self.funcs.len()];
        let mut called = vec![false; self.funcs.len()];
        let mut work: Vec<FuncId> = world
            .exports
            .iter()
            .flat_map(|&i| self.interfaces[i].funcs.iter().copied())
            .collect();
        while let Some(f) = work.pop() {
            if std::mem::replace(&mut used[f], true) {
                continue;
            }
            visit_stmts(&self.funcs[f].body, &mut |e| {
                if let ExprKind::Call { func, .. } = e.kind {
                    let iface = self.funcs[func].interface;
                    if world.imports.contains(&iface) || self.interfaces[iface].foreign.is_some() {
                        called[func] = true;
                    } else if !used[func] {
                        work.push(func);
                    }
                }
            });
        }
        let called = (0..self.funcs.len()).filter(|&f| called[f]).collect();
        (used, called)
    }
}

/// Visits every expression of a list of statements.
pub fn visit_stmts(stmts: &[Stmt], f: &mut impl FnMut(&Expr)) {
    for stmt in stmts {
        match stmt {
            Stmt::Local { init, .. } => init.iter().for_each(|e| visit_expr(e, f)),
            Stmt::Destructure { init, .. } => visit_expr(init, f),
            Stmt::Assign { place, value } => {
                match place {
                    Place::Local(_) | Place::Global(_) => {}
                    Place::Field { record, .. } => visit_expr(record, f),
                    Place::Index { list, index } => {
                        visit_expr(list, f);
                        visit_expr(index, f);
                    }
                }
                visit_expr(value, f);
            }
            Stmt::Match {
                scrutinee,
                arms,
                else_,
            } => {
                visit_expr(scrutinee, f);
                for arm in arms {
                    visit_stmts(&arm.body, f);
                }
                if let Some(else_) = else_ {
                    visit_stmts(else_, f);
                }
            }
            Stmt::If { cond, then, else_ } => {
                visit_expr(cond, f);
                visit_stmts(then, f);
                visit_stmts(else_, f);
            }
            Stmt::While { cond, body } => {
                visit_expr(cond, f);
                visit_stmts(body, f);
            }
            Stmt::Block(body) => visit_stmts(body, f),
            Stmt::Return(value) => value.iter().for_each(|e| visit_expr(e, f)),
            Stmt::TaskReturn(e) | Stmt::Call(e) => visit_expr(e, f),
            Stmt::Break | Stmt::Continue => {}
        }
    }
}

/// Visits an expression and every expression inside it.
pub fn visit_expr(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    match &e.kind {
        ExprKind::Call { args, .. }
        | ExprKind::Builtin { args, .. }
        | ExprKind::Record(args)
        | ExprKind::List(args)
        | ExprKind::Tuple(args) => args.iter().for_each(|a| visit_expr(a, f)),
        ExprKind::Unary { expr, .. }
        | ExprKind::Convert(expr)
        | ExprKind::Some(expr)
        | ExprKind::Field { expr, .. }
        | ExprKind::Elem { expr, .. }
        | ExprKind::Flag { expr, .. }
        | ExprKind::Try { expr, .. } => visit_expr(expr, f),
        ExprKind::Binary { lhs: a, rhs: b, .. } | ExprKind::Index { expr: a, index: b } => {
            visit_expr(a, f);
            visit_expr(b, f);
        }
        ExprKind::Case { payload, .. } => payload.iter().for_each(|p| visit_expr(p, f)),
        ExprKind::Const(_)
        | ExprKind::Local(_)
        | ExprKind::Move(_)
        | ExprKind::Global(_)
        | ExprKind::Str(_)
        | ExprKind::None => {}
    }
}

#[derive(Debug)]
pub struct GlobalDef {
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
    /// A constant expression (§ 8.18).
    pub init: Expr,
}

#[derive(Debug)]
pub struct InterfaceDef {
    /// The name code uses: the last part of the name for an interface of another
    /// package (§ 2.5).
    pub name: String,
    pub funcs: Vec<FuncId>,
    /// Declared types, in declaration order. Empty for an interface of another package.
    pub types: Vec<DeclId>,
    /// For an interface of another package, where it comes from.
    pub foreign: Option<Foreign>,
    /// For the implementation of an interface of another package in a world (§ 2.4.2),
    /// that interface.
    pub implements: Option<InterfaceId>,
}

/// An interface of another package, from the WIT of the dependencies (§ 3.5).
#[derive(Debug, Clone)]
pub struct Foreign {
    /// The id of the interface in [`crate::Dependencies::resolve`].
    pub id: wit_parser::InterfaceId,
    /// The WIT name of its package, `wasi:cli`.
    pub package: String,
    /// The WIT name of the interface, `stdout`.
    pub name: String,
    /// The version of its package, if it has one.
    pub version: Option<String>,
}

impl Foreign {
    /// The WIT name with the version: `wasi:cli/stdout@0.2.6`.
    pub fn wit_name(&self) -> String {
        match &self.version {
            Some(v) => format!("{}/{}@{v}", self.package, self.name),
            None => format!("{}/{}", self.package, self.name),
        }
    }

    /// The name as written in WaSL: `wasi:cli/stdout`.
    pub fn display(&self) -> String {
        format!("{}/{}", self.package, self.name).replace('-', "_")
    }
}

#[derive(Debug)]
pub struct WorldDef {
    pub name: String,
    pub exports: Vec<InterfaceId>,
    /// Imported interfaces: of the package (§ 2.5) or of other packages.
    pub imports: Vec<InterfaceId>,
    /// The imports (`true`) and exports, in source order.
    pub items: Vec<(bool, InterfaceId)>,
}

/// What a function is: a function of an interface, or a function of a resource
/// (§ 7.12), whose type is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuncKind {
    Free,
    Constructor(DeclId),
    /// Its first parameter is `self`, a `borrow` of the resource.
    Method(DeclId),
    Static(DeclId),
}

#[derive(Debug)]
pub struct FuncDef {
    pub name: String,
    /// For a function of another package, or its implementation in a world (§ 2.4.2), its
    /// name in the WIT of the dependency, which may have uppercase letters (§ 13.2).
    pub wit: Option<String>,
    pub kind: FuncKind,
    /// `async func` (§ 2.8): its calls may wait, and only an async function calls it.
    pub is_async: bool,
    /// Its body contains `task::return` (§ 8.21): an entry point, which delivers its
    /// result with `task.return` and goes on running. Its core function has no result.
    pub delivers: bool,
    pub interface: InterfaceId,
    pub params: Vec<(String, Type)>,
    pub ret: Option<Type>,
    /// Types of all the variables of the function: parameters first, then locals.
    pub locals: Vec<Type>,
    pub body: Vec<Stmt>,
}

impl FuncDef {
    /// The name of the function in WIT: its spelling in the WIT of its dependency, or its
    /// WaSL name in kebab case (§ 13.2).
    pub fn wit_name(&self) -> String {
        self.wit
            .clone()
            .unwrap_or_else(|| crate::wit::kebab(&self.name))
    }
}

#[derive(Debug)]
pub enum Stmt {
    /// Local declaration; without an initial value, the local is zero.
    Local {
        local: LocalId,
        init: Option<Expr>,
    },
    /// `local (a: A, _) = e;` (§ 9.4): the tuple `init`, whose elements go to their
    /// locals; the elements written `_` (`None`) are released at once.
    Destructure {
        locals: Vec<Option<LocalId>>,
        init: Expr,
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
    /// Statements in a block of their own. The analysis writes a `for` (§ 9.10) as the
    /// block of its hidden locals and its `while`.
    Block(Vec<Stmt>),
    /// `match` (§ 8.16). Exactly one arm or the `else` block runs.
    Match {
        scrutinee: Expr,
        arms: Vec<MatchArm>,
        /// Runs for the cases without an arm; empty when every case has one.
        else_: Option<Vec<Stmt>>,
    },
    Break,
    Continue,
    Return(Option<Expr>),
    /// `task::return(e);` (§ 8.21), at the top level of the body of a function that
    /// [delivers](FuncDef::delivers) its result.
    TaskReturn(Expr),
    /// Function call used as a statement; its result, if any, is discarded.
    Call(Expr),
}

#[derive(Debug)]
pub struct MatchArm {
    /// Index of the case in its type (see [`case_index`]).
    pub case: u32,
    /// Local that receives the payload.
    pub binding: Option<LocalId>,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub enum Place {
    Local(LocalId),
    Global(GlobalId),
    /// Field `field` of the record `record`.
    Field {
        record: Expr,
        field: u32,
    },
    /// Element `index` of the `list<mut T>` `list`.
    Index {
        list: Expr,
        index: Expr,
    },
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
    /// A local of a resource type used as a value that leaves it (§ 9.9): returned,
    /// passed for a parameter that is not a `borrow`, assigned, put in a case. Where the
    /// resource is a handle, the local is emptied.
    Move(LocalId),
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
    /// String literal: its UTF-8 bytes.
    Str(String),
    /// Record literal: the values of the fields, in declaration order.
    Record(Vec<Expr>),
    /// List literal.
    List(Vec<Expr>),
    /// `tuple(…)`.
    Tuple(Vec<Expr>),
    /// A case of a variant or a result, with its payload if the case has one. Enum cases
    /// are constants.
    Case {
        case: u32,
        payload: Option<Box<Expr>>,
    },
    /// `option::some(x)`.
    Some(Box<Expr>),
    /// `option::none`.
    None,
    /// Field of a record.
    Field {
        expr: Box<Expr>,
        field: u32,
    },
    /// Element of a tuple.
    Elem {
        expr: Box<Expr>,
        index: u32,
    },
    /// Whether flag number `flag` is in a flags value.
    Flag {
        expr: Box<Expr>,
        flag: u32,
    },
    /// Element of a list.
    Index {
        expr: Box<Expr>,
        index: Box<Expr>,
    },
    /// `expr?` (§ 8.19): the payload of `ok` or `some`, or a return from the function,
    /// whose return type is `ret`, with its `err` or `none`. The expression has no type
    /// when `ok` has no payload.
    Try {
        expr: Box<Expr>,
        ret: Type,
    },
    /// Call to a built-in function (§ 8.17).
    Builtin {
        func: Builtin,
        args: Vec<Expr>,
    },
}

/// Built-in functions (§ 8.17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    StringLen,
    StringBytes,
    StringFromBytes,
    StringChars,
    StringFromChars,
    CharFromU32,
    ListLen,
    ListFilled,
    ListCopy,
    ListFreeze,
    /// `T::wrapping_add`, `wrapping_sub`, `wrapping_mul`, `wrapping_neg`: the result
    /// modulo 2ⁿ (§ 10.2).
    Wrapping(Arith),
    /// `T::checked_add`, `checked_sub`, `checked_mul`: `option::none` on overflow.
    Checked(Arith),
    /// `T::wrapping_from(x)`: the low-order bits of the integer `x`.
    WrappingFrom,
    /// `stream::new()` or `future::new()` (§ 8.21): the tuple of the readable end and the
    /// writable end of a new stream or future, given by the type of the expression.
    EndNew,
    /// `stream::write(tx, xs)`: borrows `tx`, writes every element, returns their count.
    StreamWrite,
    /// `stream::read(rx, n)`: borrows `rx`, reads 1 to `n` elements, or `none` at the end.
    StreamRead,
    /// `stream::close(tx)`: moves `tx` and releases it.
    StreamClose,
    /// `future::write(tx, v)`: moves `tx`, gives `v` to the reader.
    FutureWrite,
    /// `future::read(rx)`: moves `rx`, returns the value of the future.
    FutureRead,
}

impl Builtin {
    /// Whether a call to the function waits (§ 8.21): only an async function makes it
    /// (E0416).
    /// Whether this is a function of streams and futures (§ 8.21).
    pub fn is_end(self) -> bool {
        matches!(
            self,
            Builtin::EndNew
                | Builtin::StreamWrite
                | Builtin::StreamRead
                | Builtin::StreamClose
                | Builtin::FutureWrite
                | Builtin::FutureRead
        )
    }

    pub fn is_async(self) -> bool {
        matches!(
            self,
            Builtin::StreamWrite | Builtin::StreamRead | Builtin::FutureWrite | Builtin::FutureRead
        )
    }
}

/// Integer arithmetic that can overflow (§ 10.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Neg,
}

impl Builtin {
    pub fn from_path(head: &str, name: &str) -> Option<Builtin> {
        if let Some(prim) = Prim::from_keyword(head) {
            if !prim.is_int() {
                return None;
            }
            return Some(match name {
                "wrapping_add" => Builtin::Wrapping(Arith::Add),
                "wrapping_sub" => Builtin::Wrapping(Arith::Sub),
                "wrapping_mul" => Builtin::Wrapping(Arith::Mul),
                "wrapping_neg" if prim.is_signed() => Builtin::Wrapping(Arith::Neg),
                "wrapping_from" => Builtin::WrappingFrom,
                "checked_add" => Builtin::Checked(Arith::Add),
                "checked_sub" => Builtin::Checked(Arith::Sub),
                "checked_mul" => Builtin::Checked(Arith::Mul),
                _ => return None,
            });
        }
        Some(match (head, name) {
            ("string", "len") => Builtin::StringLen,
            ("string", "bytes") => Builtin::StringBytes,
            ("string", "from_bytes") => Builtin::StringFromBytes,
            ("string", "chars") => Builtin::StringChars,
            ("string", "from_chars") => Builtin::StringFromChars,
            ("char", "from_u32") => Builtin::CharFromU32,
            ("list", "len") => Builtin::ListLen,
            ("list", "filled") => Builtin::ListFilled,
            ("list", "copy") => Builtin::ListCopy,
            ("list", "freeze") => Builtin::ListFreeze,
            ("stream" | "future", "new") => Builtin::EndNew,
            ("stream", "write") => Builtin::StreamWrite,
            ("stream", "read") => Builtin::StreamRead,
            ("stream", "close") => Builtin::StreamClose,
            ("future", "write") => Builtin::FutureWrite,
            ("future", "read") => Builtin::FutureRead,
            _ => return None,
        })
    }
}

/// Index of the cases of `option` and `result`, in the order of WIT.
pub const NONE: u32 = 0;
pub const SOME: u32 = 1;
pub const OK: u32 = 0;
pub const ERR: u32 = 1;

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
