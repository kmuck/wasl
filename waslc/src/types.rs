//! Types of the language: primitive types (§ 7) and the types of v0.2 (§ v2.2).
//!
//! Types are interned in a [`Types`] table: a [`Type`] is a small copyable handle, and two
//! handles are equal exactly when they designate the same type.

use std::collections::HashMap;
use std::fmt;

use wasm_encoder::ValType;

/// Primitive type of v0.1 (§ 7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prim {
    Bool,
    S8,
    S16,
    S32,
    S64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
}

impl Prim {
    pub const ALL: [Prim; 11] = [
        Prim::Bool,
        Prim::S8,
        Prim::S16,
        Prim::S32,
        Prim::S64,
        Prim::U8,
        Prim::U16,
        Prim::U32,
        Prim::U64,
        Prim::F32,
        Prim::F64,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Prim::Bool => "bool",
            Prim::S8 => "s8",
            Prim::S16 => "s16",
            Prim::S32 => "s32",
            Prim::S64 => "s64",
            Prim::U8 => "u8",
            Prim::U16 => "u16",
            Prim::U32 => "u32",
            Prim::U64 => "u64",
            Prim::F32 => "f32",
            Prim::F64 => "f64",
        }
    }

    pub fn from_keyword(word: &str) -> Option<Prim> {
        Some(match word {
            "bool" => Prim::Bool,
            "s8" => Prim::S8,
            "s16" => Prim::S16,
            "s32" => Prim::S32,
            "s64" => Prim::S64,
            "u8" => Prim::U8,
            "u16" => Prim::U16,
            "u32" => Prim::U32,
            "u64" => Prim::U64,
            "f32" => Prim::F32,
            "f64" => Prim::F64,
            _ => return None,
        })
    }

    pub fn is_signed(self) -> bool {
        matches!(self, Prim::S8 | Prim::S16 | Prim::S32 | Prim::S64)
    }

    pub fn is_unsigned(self) -> bool {
        matches!(self, Prim::U8 | Prim::U16 | Prim::U32 | Prim::U64)
    }

    pub fn is_int(self) -> bool {
        self.is_signed() || self.is_unsigned()
    }

    pub fn is_float(self) -> bool {
        matches!(self, Prim::F32 | Prim::F64)
    }

    pub fn is_numeric(self) -> bool {
        self.is_int() || self.is_float()
    }

    /// Size in bits (1 for `bool`).
    pub fn bits(self) -> u32 {
        match self {
            Prim::Bool => 1,
            Prim::S8 | Prim::U8 => 8,
            Prim::S16 | Prim::U16 => 16,
            Prim::S32 | Prim::U32 | Prim::F32 => 32,
            Prim::S64 | Prim::U64 | Prim::F64 => 64,
        }
    }

    /// Integer narrower than the `i32` it is stored in (§ 12.2).
    pub fn is_small_int(self) -> bool {
        self.is_int() && self.bits() < 32
    }

    /// Bounds of an integer type, as `i128`.
    pub fn int_range(self) -> (i128, i128) {
        let bits = self.bits();
        if self.is_signed() {
            (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
        } else {
            (0, (1i128 << bits) - 1)
        }
    }

    /// Wasm value type used to represent this type.
    pub fn val_type(self) -> ValType {
        match self {
            Prim::Bool | Prim::S8 | Prim::S16 | Prim::S32 | Prim::U8 | Prim::U16 | Prim::U32 => {
                ValType::I32
            }
            Prim::S64 | Prim::U64 => ValType::I64,
            Prim::F32 => ValType::F32,
            Prim::F64 => ValType::F64,
        }
    }
}

impl fmt::Display for Prim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Handle to a type of a [`Types`] table.
///
/// The primitive types, `char` and `string` have fixed handles, so they can be written as
/// constants and used in patterns: `Type::U32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Type(u32);

impl Type {
    pub const BOOL: Type = Type(0);
    pub const S8: Type = Type(1);
    pub const S16: Type = Type(2);
    pub const S32: Type = Type(3);
    pub const S64: Type = Type(4);
    pub const U8: Type = Type(5);
    pub const U16: Type = Type(6);
    pub const U32: Type = Type(7);
    pub const U64: Type = Type(8);
    pub const F32: Type = Type(9);
    pub const F64: Type = Type(10);
    pub const CHAR: Type = Type(11);
    pub const STRING: Type = Type(12);

    pub fn prim(p: Prim) -> Type {
        Type(
            Prim::ALL
                .iter()
                .position(|&q| q == p)
                .expect("primitive type") as u32,
        )
    }

    /// The primitive type this handle designates, if any.
    pub fn as_prim(self) -> Option<Prim> {
        Prim::ALL.get(self.0 as usize).copied()
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn is_signed(self) -> bool {
        self.as_prim().is_some_and(Prim::is_signed)
    }

    pub fn is_unsigned(self) -> bool {
        self.as_prim().is_some_and(Prim::is_unsigned)
    }

    pub fn is_int(self) -> bool {
        self.as_prim().is_some_and(Prim::is_int)
    }

    pub fn is_float(self) -> bool {
        self.as_prim().is_some_and(Prim::is_float)
    }

    pub fn is_numeric(self) -> bool {
        self.as_prim().is_some_and(Prim::is_numeric)
    }

    pub fn is_small_int(self) -> bool {
        self.as_prim().is_some_and(Prim::is_small_int)
    }

    /// Size in bits of a primitive type.
    pub fn bits(self) -> u32 {
        self.as_prim().expect("primitive type").bits()
    }

    pub fn int_range(self) -> (i128, i128) {
        self.as_prim().expect("integer type").int_range()
    }

    /// Wasm value type of a primitive type.
    pub fn val_type(self) -> ValType {
        self.as_prim().expect("primitive type").val_type()
    }
}

/// Index of a declared type in [`Types::decls`].
pub type DeclId = usize;

/// What a type is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeKind {
    Prim(Prim),
    Char,
    String,
    List {
        mutable: bool,
        elem: Type,
    },
    Tuple(Vec<Type>),
    Option(Type),
    Result {
        ok: Option<Type>,
        err: Option<Type>,
    },
    /// A `record`, `enum`, `variant` or `flags` type declared in an interface.
    Decl(DeclId),
}

/// A type declared in an interface (§ v2.2.3).
#[derive(Debug)]
pub struct TypeDecl {
    pub name: String,
    /// Index of the interface that declares it.
    pub interface: usize,
    pub kind: DeclKind,
}

#[derive(Debug)]
pub enum DeclKind {
    Record(Vec<FieldDef>),
    Enum(Vec<String>),
    Variant(Vec<CaseDef>),
    Flags(Vec<String>),
}

impl DeclKind {
    pub fn keyword(&self) -> &'static str {
        match self {
            DeclKind::Record(_) => "record",
            DeclKind::Enum(_) => "enum",
            DeclKind::Variant(_) => "variant",
            DeclKind::Flags(_) => "flags",
        }
    }
}

#[derive(Debug)]
pub struct FieldDef {
    pub name: String,
    pub mutable: bool,
    pub ty: Type,
}

#[derive(Debug)]
pub struct CaseDef {
    pub name: String,
    pub payload: Option<Type>,
}

/// The types of a program: interned type expressions and declared types.
#[derive(Debug)]
pub struct Types {
    kinds: Vec<TypeKind>,
    ids: HashMap<TypeKind, Type>,
    pub decls: Vec<TypeDecl>,
    /// Interface names, to display declared types with their path.
    pub interface_names: Vec<String>,
}

impl Default for Types {
    fn default() -> Self {
        let mut types = Types {
            kinds: Vec::new(),
            ids: HashMap::new(),
            decls: Vec::new(),
            interface_names: Vec::new(),
        };
        for p in Prim::ALL {
            types.intern(TypeKind::Prim(p));
        }
        assert_eq!(types.intern(TypeKind::Char), Type::CHAR);
        assert_eq!(types.intern(TypeKind::String), Type::STRING);
        types
    }
}

impl Types {
    /// The handle of a type, created if needed.
    pub fn intern(&mut self, kind: TypeKind) -> Type {
        if let Some(&ty) = self.ids.get(&kind) {
            return ty;
        }
        let ty = Type(self.kinds.len() as u32);
        self.kinds.push(kind.clone());
        self.ids.insert(kind, ty);
        ty
    }

    pub fn kind(&self, ty: Type) -> &TypeKind {
        &self.kinds[ty.index()]
    }

    /// Every type of the table, in creation order.
    pub fn all(&self) -> impl Iterator<Item = Type> + use<> {
        (0..self.kinds.len() as u32).map(Type)
    }

    /// The declaration of a declared type.
    pub fn decl(&self, ty: Type) -> Option<(DeclId, &TypeDecl)> {
        match self.kind(ty) {
            TypeKind::Decl(id) => Some((*id, &self.decls[*id])),
            _ => None,
        }
    }

    /// The type of a declaration.
    pub fn decl_type(&mut self, id: DeclId) -> Type {
        self.intern(TypeKind::Decl(id))
    }

    /// The type of a declaration that is already interned.
    pub fn decl_type_of(&self, id: DeclId) -> Type {
        self.ids[&TypeKind::Decl(id)]
    }

    /// The types a type is directly made of: elements, fields, payloads.
    pub fn children(&self, ty: Type) -> Vec<Type> {
        match self.kind(ty) {
            TypeKind::Prim(_) | TypeKind::Char | TypeKind::String => Vec::new(),
            TypeKind::List { elem, .. } => vec![*elem],
            TypeKind::Tuple(elems) => elems.clone(),
            TypeKind::Option(inner) => vec![*inner],
            TypeKind::Result { ok, err } => ok.iter().chain(err).copied().collect(),
            TypeKind::Decl(id) => match &self.decls[*id].kind {
                DeclKind::Record(fields) => fields.iter().map(|f| f.ty).collect(),
                DeclKind::Variant(cases) => cases.iter().filter_map(|c| c.payload).collect(),
                DeclKind::Enum(_) | DeclKind::Flags(_) => Vec::new(),
            },
        }
    }

    /// Every declared type reachable from `ty`, `ty` included, in a deterministic order.
    pub fn reachable_decls(&self, ty: Type) -> Vec<DeclId> {
        let mut seen = Vec::new();
        let mut decls = Vec::new();
        let mut stack = vec![ty];
        while let Some(t) = stack.pop() {
            if seen.contains(&t) {
                continue;
            }
            seen.push(t);
            if let TypeKind::Decl(id) = self.kind(t) {
                decls.push(*id);
            }
            let mut children = self.children(t);
            children.reverse();
            stack.extend(children);
        }
        decls
    }

    /// Whether a declared type is part of a cycle of types (§ v2.2.5).
    pub fn is_recursive(&self, id: DeclId) -> bool {
        let start = self.decl_type_of(id);
        let mut seen = Vec::new();
        let mut stack = self.children(start);
        while let Some(t) = stack.pop() {
            if t == start {
                return true;
            }
            if seen.contains(&t) {
                continue;
            }
            seen.push(t);
            stack.extend(self.children(t));
        }
        false
    }

    /// Whether the type is a scalar: a primitive type, `char`, an enum or a flags type
    /// (§ v2.2.1).
    pub fn is_scalar(&self, ty: Type) -> bool {
        match self.kind(ty) {
            TypeKind::Prim(_) | TypeKind::Char => true,
            TypeKind::Decl(id) => {
                matches!(self.decls[*id].kind, DeclKind::Enum(_) | DeclKind::Flags(_))
            }
            _ => false,
        }
    }

    /// Displays a type as written in WaSL: `list<mut u8>`, `invoices::line`.
    pub fn display(&self, ty: Type) -> String {
        self.display_in(ty, None)
    }

    /// Displays a type as written inside interface `iface`, where its own types have no
    /// path: `line` rather than `invoices::line`.
    pub fn display_in(&self, ty: Type, iface: Option<usize>) -> String {
        match self.kind(ty) {
            TypeKind::Prim(p) => p.name().to_string(),
            TypeKind::Char => "char".into(),
            TypeKind::String => "string".into(),
            TypeKind::List { mutable, elem } => format!(
                "list<{}{}>",
                if *mutable { "mut " } else { "" },
                self.display_in(*elem, iface)
            ),
            TypeKind::Tuple(elems) => format!(
                "tuple<{}>",
                elems
                    .iter()
                    .map(|&t| self.display_in(t, iface))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            TypeKind::Option(inner) => format!("option<{}>", self.display_in(*inner, iface)),
            TypeKind::Result { ok, err } => match (ok, err) {
                (None, None) => "result".into(),
                (Some(ok), None) => format!("result<{}>", self.display_in(*ok, iface)),
                (None, Some(err)) => format!("result<_, {}>", self.display_in(*err, iface)),
                (Some(ok), Some(err)) => {
                    format!(
                        "result<{}, {}>",
                        self.display_in(*ok, iface),
                        self.display_in(*err, iface)
                    )
                }
            },
            TypeKind::Decl(id) => {
                let decl = &self.decls[*id];
                match self.interface_names.get(decl.interface) {
                    Some(name) if iface != Some(decl.interface) => {
                        format!("{name}::{}", decl.name)
                    }
                    _ => decl.name.clone(),
                }
            }
        }
    }
}
