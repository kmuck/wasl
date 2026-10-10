# 7. Types

## 7.1 Primitive types

v0.1 has eleven types. They are exactly the primitive types of WIT with the same names, and each one is represented by a core Wasm value type.

| Type | Values | Wasm representation |
|---|---|---|
| `bool` | `true`, `false` | `i32` (0 or 1) |
| `s8` | −128 … 127 | `i32` |
| `s16` | −32,768 … 32,767 | `i32` |
| `s32` | −2³¹ … 2³¹−1 | `i32` |
| `s64` | −2⁶³ … 2⁶³−1 | `i64` |
| `u8` | 0 … 255 | `i32` |
| `u16` | 0 … 65,535 | `i32` |
| `u32` | 0 … 2³²−1 | `i32` |
| `u64` | 0 … 2⁶⁴−1 | `i64` |
| `f32` | IEEE 754 binary32 (single precision) | `f32` |
| `f64` | IEEE 754 binary64 (double precision) | `f64` |

The representation of 8- and 16-bit integers inside an `i32` is specified in § 12.2.

## 7.2 Categories

The typing rules refer to these categories:

- **signed integers**: `s8`, `s16`, `s32`, `s64`;
- **unsigned integers**: `u8`, `u16`, `u32`, `u64`;
- **integers**: signed and unsigned integers;
- **floats**: `f32`, `f64`;
- **numeric types**: integers and floats.

`bool` is in none of these categories.

## 7.3 Distinct types

All types are distinct. No type converts implicitly to another: a `u32` cannot be used where a `u64` is expected, an integer cannot be used as a condition, a `bool` is not a number. Conversions between numeric types are written explicitly (§ 8.5).

## 7.4 Functions without a value

A function without a return type returns no value. A call to such a function is not an expression of any type: it can only be used as a statement (§ 9.6). There is no `void` or unit type that can be written.
