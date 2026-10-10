//! Primitive types of the language (§ 7).

use std::fmt;

use wasm_encoder::ValType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
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

impl Type {
    pub fn name(self) -> &'static str {
        match self {
            Type::Bool => "bool",
            Type::S8 => "s8",
            Type::S16 => "s16",
            Type::S32 => "s32",
            Type::S64 => "s64",
            Type::U8 => "u8",
            Type::U16 => "u16",
            Type::U32 => "u32",
            Type::U64 => "u64",
            Type::F32 => "f32",
            Type::F64 => "f64",
        }
    }

    pub fn from_keyword(word: &str) -> Option<Type> {
        Some(match word {
            "bool" => Type::Bool,
            "s8" => Type::S8,
            "s16" => Type::S16,
            "s32" => Type::S32,
            "s64" => Type::S64,
            "u8" => Type::U8,
            "u16" => Type::U16,
            "u32" => Type::U32,
            "u64" => Type::U64,
            "f32" => Type::F32,
            "f64" => Type::F64,
            _ => return None,
        })
    }

    pub fn is_signed(self) -> bool {
        matches!(self, Type::S8 | Type::S16 | Type::S32 | Type::S64)
    }

    pub fn is_unsigned(self) -> bool {
        matches!(self, Type::U8 | Type::U16 | Type::U32 | Type::U64)
    }

    pub fn is_int(self) -> bool {
        self.is_signed() || self.is_unsigned()
    }

    pub fn is_float(self) -> bool {
        matches!(self, Type::F32 | Type::F64)
    }

    pub fn is_numeric(self) -> bool {
        self.is_int() || self.is_float()
    }

    /// Size in bits (1 for `bool`).
    pub fn bits(self) -> u32 {
        match self {
            Type::Bool => 1,
            Type::S8 | Type::U8 => 8,
            Type::S16 | Type::U16 => 16,
            Type::S32 | Type::U32 | Type::F32 => 32,
            Type::S64 | Type::U64 | Type::F64 => 64,
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
            Type::Bool | Type::S8 | Type::S16 | Type::S32 | Type::U8 | Type::U16 | Type::U32 => {
                ValType::I32
            }
            Type::S64 | Type::U64 => ValType::I64,
            Type::F32 => ValType::F32,
            Type::F64 => ValType::F64,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
