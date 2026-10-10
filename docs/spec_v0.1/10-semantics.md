# 10. Runtime semantics

This chapter defines what a valid program computes. Most rules are those of WebAssembly itself (§ 1.2, principle 1).

## 10.1 Evaluation order

- The operands of a binary operator are evaluated from left to right.
- The arguments of a call are evaluated from left to right, then the function is called.
- `&&` and `||` short-circuit: the right operand is only evaluated if the left one does not decide the result (`false && …` and `true || …` do not evaluate their right operand).
- In an assignment, the value is evaluated, then stored.

## 10.2 Integers

**Wrapping arithmetic.** The result of `+`, `-`, `*` and unary `-` is reduced modulo 2ⁿ, where n is the bit width of the type: integer overflow wraps around and never traps.

| Type | Expression | Result |
|---|---|---|
| `u8` | `255 + 1` | `0` |
| `u8` | `200 + 100` | `44` |
| `s8` | `127 + 1` | `-128` |
| `s8` | `-(-128)` | `-128` |
| `u16` | `300 * 300` | `24464` |
| `u32` | `0 - 1` | `4294967295` |
| `s64` | `9223372036854775807 + 1` | `-9223372036854775808` |

**Division `/`.** Truncated toward zero: `-7 / 2 = -3`, `7 / -2 = -3`.

- Division by zero: **trap**.
- For a signed type, `MIN / -1` (whose exact result does not fit in the type, e.g. `-128 / -1` in `s8`): **trap**, for every signed type, including `s8` and `s16`.

**Remainder `%`.** The result has the sign of the dividend: `-7 % 2 = -1`, `7 % -2 = 1`.

- Remainder by zero: **trap**.
- `MIN % -1 = 0` (no trap).

**Bitwise operators** `&`, `|`, `^` operate on the bits of the value.

**Shifts.**

- `<<` shifts left, inserting zeros.
- `>>` is an arithmetic shift for a signed type (the sign bit is copied: `-128 >> 1 = -64` in `s8`) and a logical shift for an unsigned type (zeros are inserted).
- The shift amount is the right operand, interpreted as unsigned and **taken modulo the bit width of the type**: for a `u8`, `x << 9` is `x << 1`; for a `u32`, `x << 33` is `x << 1`. The bits shifted out of the type are lost (`u8`: `129 << 1 = 2`).

**Comparisons** use the signed order for signed types and the unsigned order for unsigned types: with `u32`, `1 < 4294967295` is `true`; with `s32`, `1 < -1` is `false`.

## 10.3 Floats

- Arithmetic follows IEEE 754, with rounding to nearest, ties to even.
- Division by zero does not trap: `1.0 / 0.0 = +inf`, `-1.0 / 0.0 = -inf`, `0.0 / 0.0 = NaN`.
- Comparisons follow IEEE 754: any comparison involving NaN is `false`, except `!=`, which is `true`. In particular `x != x` is `true` exactly when `x` is NaN.
- Unary `-` flips the sign bit (`-(0.0)` is negative zero).

## 10.4 Booleans

`!` negates. `==` and `!=` compare two booleans. `&&` and `||` are described in § 10.1.

## 10.5 Conversions

Let `T(e)` be a conversion where `e` has type `S`. In the examples, `S` value → `T` result reads: converting a value of type `S` to type `T` gives this result.

| Case | Result | Examples |
|---|---|---|
| `S` = `T` | `e`, unchanged | |
| integer → integer with **more** bits | the value is extended: with its sign if `S` is signed, with zeros if `S` is unsigned. The result is then read as a `T`. | `s8` −1 → `u64` 18446744073709551615; `s8` −1 → `u16` 65535; `u8` 200 → `s32` 200 |
| integer → integer with **as many or fewer** bits | the low-order bits of `e` are kept and read as a `T` | `u32` 300 → `u8` 44; `u8` 200 → `s8` −56; `s32` −1 → `u32` 4294967295 |
| integer → float | the nearest float value | `u64` 18446744073709551615 → `f64` 18446744073709551616.0; `s16` −5 → `f64` −5.0 |
| float → integer | truncation toward zero, then **saturation**: NaN gives 0, a value below the minimum of `T` gives the minimum, a value above the maximum gives the maximum | `f64` −3.9 → `s32` −3; `f64` 3.9 → `u8` 3; `f64` 300.7 → `u8` 255; `f64` −1.0 → `u8` 0; `f64` −1e10 → `s8` −128; `f64` NaN → `u32` 0; `f32` +inf → `u64` 18446744073709551615 |
| `f64` → `f32` | the nearest `f32` value; ± infinity if the value is out of range | `f64` 0.1 → the `f32` nearest to 0.1 |
| `f32` → `f64` | the exact same value | |

Float to integer conversions never trap.

## 10.6 Calls and globals

- Calling a function evaluates its body with its parameters bound to the arguments. Recursion is allowed.
- Globals keep their value for the lifetime of the instance of the component (§ 6.4). Each call to an exported function on the same instance sees the changes made by the previous calls.

## 10.7 Traps

A **trap** stops the current call immediately. The host receives an error and the instance of the component must not be used again. The causes of a trap in v0.1 are:

| Cause | Message reported by wasmtime |
|---|---|
| integer division or remainder by zero | `integer divide by zero` |
| signed `MIN / -1` | `integer overflow` |
| call stack exhaustion (recursion too deep) | `call stack exhausted` |

Nothing else traps in v0.1: overflow wraps, float operations and conversions never trap.
