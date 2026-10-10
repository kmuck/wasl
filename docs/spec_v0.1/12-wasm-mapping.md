# 12. Mapping to WebAssembly

This chapter specifies the **core Wasm module** that the compiler generates for a world. The component wraps this module (chapter 13). The generated code must produce the semantics of chapter 10; the exact instructions below are those of the reference implementation, and any other sequence with the same behavior conforms.

## 12.1 Content of the module

For a world `w`, the module contains:

- **functions**: every function of the interfaces exported by `w`, plus every function they call, directly or transitively, whatever its interface. Other functions of the package are not included;
- **globals**: every global read or assigned by those functions;
- **exports**: one export per function of each interface exported by `w`, with the name of § 13.3;
- **types**: one function type per distinct signature.

The module has no memory, no table, no import and no start function. Functions and globals appear in the order of their declaration in the package (§ 3.3, § 11.2), restricted to those included.

| WaSL | Wasm |
|---|---|
| `global x: T = v;` | `(global T' (T'.const v))` |
| `global x: mut T = v;` | `(global (mut T') (T'.const v))` |
| function | `(func (param …) (result …) (local …) …)` |
| parameters | the parameters of the Wasm function |
| `local x: T;` | a Wasm local, set to zero (`T'.const 0` then `local.set`) every time the declaration runs |
| `local x: T = e;` | a Wasm local; `e` then `local.set` |
| `x = e;` | `e` then `local.set` or `global.set` |
| `if (c) { … } else { … }` | `c`, `if … else … end` |
| `while (c) { … }` | `block` `loop` `c` `i32.eqz` `br_if 1` `…` `br 0` `end` `end` |
| `break` | `br` to the `block` of the innermost `while` |
| `continue` | `br` to the `loop` of the innermost `while` |
| `return e;` | `e` `return` |
| `f(a, b)` | `a` `b` `call $f` |
| `f(a);` as a statement | `a` `call $f`, then `drop` if `f` returns a value |
| `a && b` | `a` `if (result i32)` `b` `else` `i32.const 0` `end` |
| `a \|\| b` | `a` `if (result i32)` `i32.const 1` `else` `b` `end` |
| `!a` | `a` `i32.eqz` |
| `-a` (integer) | `T'.const 0` `a` `T'.sub` |
| `-a` (float) | `a` `T'.neg` |

`T'` is the Wasm type of `T` (§ 7.1). Because Wasm only zeroes locals when a function starts, a local declared inside a loop is explicitly reset on every iteration.

A function with a return type ends with `unreachable`: every path returns before reaching the end (§ 8.8), but the Wasm validator needs the end of the function to be well-typed.

## 12.2 8- and 16-bit integers

Values of type `s8`, `s16`, `u8`, `u16` are stored in `i32`s that are **always normalized**:

- `s8` and `s16`: sign-extended (an `s8` −1 is the `i32` −1);
- `u8` and `u16`: zero-extended (a `u8` 255 is the `i32` 255).

After an operation whose result can leave the range of the type, the compiler normalizes the result again:

| Type | Normalization |
|---|---|
| `s8` | `i32.extend8_s` |
| `s16` | `i32.extend16_s` |
| `u8` | `i32.const 0xFF` `i32.and` |
| `u16` | `i32.const 0xFFFF` `i32.and` |

The operations that need it are `+`, `-`, `*`, unary `-`, `<<` and the conversions to these types. Thanks to this invariant, `i32` comparisons, divisions, remainders, `>>` and bitwise operations directly give the right result without normalization.

Two operations need extra code:

- **Shifts**: the shift amount is masked with `bits − 1` (`i32.const 7` `i32.and` for 8 bits) before the shift, because Wasm only masks with 31.
- **`MIN / -1` for `s8` and `s16`**: in `i32`, `-128 / -1 = 128` does not overflow. The compiler checks `b == -1 && a == MIN` and, if so, executes `i32.const 0x80000000` `i32.const -1` `i32.div_s`, which traps with the same `integer overflow` as the other signed types.

32- and 64-bit integers, `bool` and floats need no normalization.

## 12.3 Instructions per operation

The type of the operands selects the instruction:

| Operation | Signed | Unsigned | Float |
|---|---|---|---|
| `+` `-` `*` | `add` `sub` `mul` | `add` `sub` `mul` | `add` `sub` `mul` |
| `/` | `div_s` | `div_u` | `div` |
| `%` | `rem_s` | `rem_u` | — |
| `&` `\|` `^` | `and` `or` `xor` | `and` `or` `xor` | — |
| `<<` | `shl` | `shl` | — |
| `>>` | `shr_s` | `shr_u` | — |
| `==` `!=` | `eq` `ne` | `eq` `ne` | `eq` `ne` |
| `<` `<=` `>` `>=` | `lt_s` `le_s` `gt_s` `ge_s` | `lt_u` `le_u` `gt_u` `ge_u` | `lt` `le` `gt` `ge` |

with the prefix `i32.`, `i64.`, `f32.` or `f64.` of the Wasm type. `bool` uses `i32.eq` / `i32.ne`.

## 12.4 Conversions

For a conversion from `S` to `T` (§ 10.5):

| From → to | Instructions |
|---|---|
| `i32`-based integer → `i64`-based integer | `i64.extend_i32_s` if `S` is signed, `i64.extend_i32_u` otherwise |
| `i64`-based integer → `i32`-based integer | `i32.wrap_i64`, then normalization of `T` (§ 12.2) |
| `i32`-based → `i32`-based integer | normalization of `T` |
| `i64`-based → `i64`-based integer | none |
| integer → float | `f32.convert_i32_s`, `f64.convert_i64_u`, … according to the width and signedness of `S` |
| float → 32- or 64-bit integer | `i32.trunc_sat_f64_s`, `i64.trunc_sat_f32_u`, … according to the width and signedness of `T` |
| float → 8- or 16-bit integer | `i32.trunc_sat_f32_s` or `i32.trunc_sat_f64_s`, then clamping to the range of `T` with two `select`s |
| `f32` → `f64` | `f64.promote_f32` |
| `f64` → `f32` | `f32.demote_f64` |

The `trunc_sat` instructions (non-trapping float-to-int conversions, part of WebAssembly 2.0) provide the saturation of § 10.5.
