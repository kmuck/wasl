# 12. Mapping to WebAssembly and WasmGC

This chapter specifies the **core Wasm module** that the compiler generates for a world, and how each type is represented in it. The component wraps this module (chapter 13). The generated code must produce the semantics of chapter 10; the exact instructions below are those of the reference implementation, and any other sequence with the same behavior conforms.

The core module of a world that uses heap types requires the WebAssembly **GC** proposal, with the reference types and typed function references it builds on, all part of WebAssembly 3.0. `weasel` enables them in wasmtime (§ 14.6).

## 12.1 Content of the module

For a world `w`, the module contains:

- **types**: the type definitions of § 12.5 used by the included functions and globals, then one function type per distinct signature;
- **functions**: every function of the interfaces exported by `w`, plus every function they call, directly or transitively, whatever its interface. Other functions of the package are not included. To these are added the helpers they use (§ 12.8) and the boundary adapters of the exported functions (§ 13.8);
- **globals**: every global read or assigned by those functions, one immutable global per distinct string literal and per shared case without payload (§ 12.6), then the globals of § 13.7 when the module has a memory;
- **imports**: the functions of the interfaces imported by `w` that the included functions call, and the `resource.drop` of each imported resource they use, with the names of § 13.10; they come first in the function index space;
- **exports**: one export per function of each interface exported by `w`, with the name of § 13.3, and the functions of the resources of these interfaces (§ 13.11);
- **memory**, **realloc** and **post-return** functions only when an exported function needs them (§ 13.6).

The module has no table and no start function, and no import when `w` imports nothing. Functions and globals appear in the order of their declaration in the package (§ 3.3, § 11.2), restricted to those included.

| WaSL | Wasm |
|---|---|
| `global x: T = v;` | `(global T' (T'.const v))` |
| `global x: mut T = v;` | `(global (mut T') (T'.const v))` |
| function | `(func (param …) (result …) (local …) …)` |
| parameters | the parameters of the Wasm function |
| `local x: T;` | a Wasm local, set to the default value of `T'` (§ 9.4: `T'.const 0`, or `ref.null` for an option) then `local.set`, every time the declaration runs |
| `local x: T = e;` | a Wasm local; `e` then `local.set` |
| `x = e;` | `e` then `local.set` or `global.set` |
| `if (c) { … } else { … }` | `c`, `if … else … end` |
| `while (c) { … }` | `block` `loop` `c` `i32.eqz` `br_if 1` `…` `br 0` `end` `end` |
| `break` | `br` to the `block` of the innermost `while` |
| `continue` | `br` to the `loop` of the innermost `while` |
| `return e;` | `e` `return` |
| `f(a, b)` | `a` `b` `call $f` |
| `f(a);` as a statement | `a` `call $f` (`f` returns no value, § 9.6) |
| `_ = f(a);` | `a` `call $f` `drop` |
| `a && b` | `a` `if (result i32)` `b` `else` `i32.const 0` `end` |
| `a \|\| b` | `a` `if (result i32)` `i32.const 1` `else` `b` `end` |
| `!a` | `a` `i32.eqz` |
| `-a` (integer) | `T'.const 0` `a` `T'.sub`, with the overflow test of § 12.9 |
| `-a` (float) | `a` `T'.neg` |

`T'` is the Wasm type of `T` (§ 7.1, § 12.5). Because Wasm only sets locals to their default value when a function starts, a local declared inside a loop is explicitly reset on every iteration. The instructions of the other expressions and statements, on heap types, are in § 12.8.

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

The operations that need it are `<<`, and `+`, `-`, `*`, unary `-` and the conversions to these types when they wrap (the `wrapping_` functions of § 8.17); in the plain operators, the overflow test of § 12.9 replaces the normalization. Thanks to this invariant, `i32` comparisons, divisions, remainders, `>>` and bitwise operations directly give the right result without normalization.

Two operations need extra code:

- **Shifts**: the shift amount is masked with `bits − 1` (`i32.const 7` `i32.and` for 8 bits) before the shift, because Wasm only masks with 31.
- **`MIN / -1` for `s8` and `s16`**: in `i32`, `-128 / -1 = 128` does not overflow. The compiler checks `b == -1 && a == MIN` and, if so, executes `i32.const 0x80000000` `i32.const -1` `i32.div_s`, which traps with the same `integer overflow` as the other signed types.

32- and 64-bit integers, `bool` and floats need no normalization.

## 12.3 Instructions per operation

The type of the operands selects the instruction. `+`, `-` and `*` on integers are followed by the overflow test of § 12.9:

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

For a conversion from `S` to `T` (§ 10.5), the instructions below are preceded by the overflow test of § 12.9 when the value may not fit, and the float to integer conversions use the trapping `trunc` instructions given there. Without the test, these are the instructions of `T::wrapping_from` (§ 8.17):

| From → to | Instructions |
|---|---|
| `i32`-based integer → `i64`-based integer | `i64.extend_i32_s` if `S` is signed, `i64.extend_i32_u` otherwise |
| `i64`-based integer → `i32`-based integer | `i32.wrap_i64`, then normalization of `T` (§ 12.2) |
| `i32`-based → `i32`-based integer | normalization of `T` |
| `i64`-based → `i64`-based integer | none |
| integer → float | `f32.convert_i32_s`, `f64.convert_i64_u`, … according to the width and signedness of `S` |
| float → integer | the trapping `trunc` instructions of § 12.9 |
| `f32` → `f64` | `f64.promote_f32` |
| `f64` → `f32` | `f32.demote_f64` |
| `char` → `u32` | nothing |

## 12.5 Representation of the types

`T'` is the Wasm type of a WaSL type `T`:

| WaSL type | `T'` | Type definition |
|---|---|---|
| primitive types | as in § 7.1 | |
| `char` | `i32` | the code point |
| enum | `i32` | the index of the case, in declaration order from 0 |
| flags | `i32` | bit `k` set when the flag `k` (in declaration order, from 0) is in the set |
| `string` | `(ref $bytes)` | `$bytes = (array (mut i8))` |
| `list<T>`, `list<mut T>` | `(ref $list_T)` | `$list_T = (array (mut E))`, with `E` below |
| `tuple<T1, …, Tn>` | `(ref $tuple)` | `$tuple = (struct (field T1') … (field Tn'))`, immutable fields |
| record `r` | `(ref $r)` | `$r = (struct (field F1) …)`, one field per WaSL field, in declaration order; a `mut` field is `(mut T')` |
| variant `v` | `(ref $v)` | see § 12.6 |
| `result<…>` | `(ref $v)` | a variant with the cases `ok` (0) and `err` (1) |
| `option<T>` | `(ref null X)` | see § 12.7 |
| resource `r` of the package | `(ref $r)` | `$r = (struct (field F1) …)`, like a record (§ 12.10) |
| handle type (§ 7.12) | `i32` | the index of the handle in the handle table of the instance |
| `borrow<r>` | the type of `r` | |

**List elements.** `E` is `i8` for `bool`, `s8` and `u8`; `i16` for `s16` and `u16`; `T'` otherwise. Packed elements are read with `array.get_s` for signed types and `array.get_u` otherwise, which gives the normalized `i32` of § 12.2.

**Same representation, distinct types.** `string`, `list<u8>`, `list<s8>` and `list<bool>` share `$bytes`; `list<T>` and `list<mut T>` share `$list_T`.

**Why immutable lists are mutable Wasm arrays today.** WasmGC can only create an immutable array (`(array T)`) with `array.new` (one value repeated), `array.new_fixed` (a number of values fixed in the code) or `array.new_data` / `array.new_elem` (the contents of a segment of the binary). Filling an array element by element, with `array.set` or `array.copy`, requires a mutable array, and a mutable array cannot be turned into an immutable one: `(array (mut T))` is not a subtype of `(array T)`. A list received at the component boundary, the result of `list::freeze` or of `string::chars`, has contents known only at run time, so it can only be built in a mutable array. Every list is therefore an `(array (mut E))`, and the immutability of `list<T>` (§ 7.10) is enforced by the type checker: no code that writes to a `list<T>` is ever generated. When WasmGC can build an immutable array from values computed at run time, `list<T>` becomes an `(array E)` without any change to the language. Strings follow the same rule and are never written after they are built; this is why `string::bytes` costs nothing.

**Record fields** use the representation of § 7.1 for scalars (an `i32` for a `u8` field), not packed fields: struct fields are few, and normalized `i32`s need no extension on reads.

**Type identity.** WasmGC types are structural: two WaSL records with the same field types may become the same Wasm type. Generated code never relies on a cast to tell two WaSL types apart; the type checker already did. Variant cases are told apart by their tag (§ 12.6), never by a cast alone.

**Recursion group.** All the GC types of the module (structs and arrays) are defined in **one** recursion group (`rec`), placed before the function types, so that types that refer to each other in a cycle (§ 7.9) need nothing special. Within a module this behaves exactly like one group per cycle: GC types never cross the boundary (chapter 13), so their canonical identity across modules does not matter.

## 12.6 Variants and results

A variant `v` is a small family of struct types:

```wasm
(type $v      (sub (struct (field i32))))                             ;; the tag: index of the case
(type $v.c    (sub final $v (struct (field i32) (field P'))))         ;; one per case with a payload P
```

- A value is a struct whose first field, the **tag**, is the index of its case in declaration order. A case with a payload is a `$v.c`; a case without payload is a `$v` with only the tag.
- A case without payload is created once per instance, in an immutable global initialized with `struct.new $v (i32.const k)`, and reused.
- `match` reads the tag with `struct.get $v 0` and dispatches with `br_table`. In the block of a case with a binding, `ref.cast (ref $v.c)` then `struct.get $v.c 1` give the payload; the cast never fails, since the tag was checked.

`result<T, E>` uses the same layout, with `ok` = 0 and `err` = 1; a form without a payload for a case (§ 7.6) has no payload field for that case.

## 12.7 Options

`option<T>` uses a nullable reference, `none` being `null`:

- if `T'` is a non-nullable reference `(ref $X)` (a string, a list, a record, a variant, a result), `option<T>` is `(ref null $X)`: `option::some(x)` is `x` itself, with no allocation;
- otherwise (`T` is a scalar, or itself an option, which is already nullable), the payload is boxed: `$box_T = (struct (field T'))`, `option<T>` is `(ref null $box_T)`, and `option::some(x)` is `struct.new $box_T`.

`match` on an option tests with `br_on_null`; the payload is the reference itself, or `struct.get $box_T 0`.

## 12.8 Instructions

| WaSL | Wasm |
|---|---|
| `local x: T = e;` (heap type) | a Wasm local of type `T'`; `e` then `local.set` |
| `local x: T;` | the default value of `T'` (§ 9.4), `i32.const 0` or `ref.null $X`, then `local.set` |
| `f(…);`, `_ = f(…);` | the call, followed by `drop` for `_ =` |
| `"abc"` | `global.get $str_k`, a global initialized with `array.new_fixed $bytes 3 (i32.const 97) (i32.const 98) (i32.const 99)`; one global per distinct literal |
| record literal `r { … }` | the values, `struct.new $r` |
| `e.f` | `e` `struct.get $r f` |
| `e.f = v;` | `e` `v` `struct.set $r f` |
| list literal `[a, b]` | `a` `b` `array.new_fixed $list_T 2` |
| `e[i]` | `e` `i` `array.get $list_T` (or `array.get_s` / `array.get_u`) |
| `e[i] = v;` | `e` `i` `v` `array.set $list_T` |
| enum case `e::c` | `i32.const k` |
| `v::c(x)` | `i32.const k` `x` `struct.new $v.c` |
| `v::c` (no payload) | `global.get` of its shared instance |
| `option::some(x)` | `x`, or `x` `struct.new $box_T` |
| `option::none` | `ref.null $X` |
| `==`, `!=` on enums, flags, chars | `i32.eq`, `i32.ne` |
| `<`, `<=`, `>`, `>=` on chars | `i32.lt_u`, `i32.le_u`, `i32.gt_u`, `i32.ge_u` |
| char literal | `i32.const` of its code point |
| flags literal `f { a, c }` | `i32.const` with the bits of `a` and `c` |
| `f.a` on flags | `f` `i32.const (1 << k)` `i32.and` then `i32.const 0` `i32.ne` |
| `\|`, `&` on flags | `i32.or`, `i32.and` |
| `tuple(a, b)` | `a` `b` `struct.new $tuple` |
| `t.k` | `t` `struct.get $tuple k` |
| `u32(c)` on a char | nothing |
| `==`, `!=` on strings | a call to a generated helper that compares the lengths, then the bytes |
| `a + b` on strings | a helper: `array.new_default $bytes` of the total length, then two `array.copy` |
| `string::len(s)`, `list::len(xs)` | `array.len` |
| `string::bytes(s)` | nothing: the same reference |
| `string::from_bytes(b)` | a helper that validates UTF-8, then copies the bytes into a new `$bytes` with `array.copy`; `ref.null` if invalid |
| `list::filled(n, x)` | `x` `n` `array.new $list_T` |
| `list::copy(xs)`, `list::freeze(xs)` | `array.new` of the same length, filled with the first element (or `array.new_fixed $list_T 0` if empty), then `array.copy` |
| `string::chars(s)`, `string::from_chars(cs)` | helpers that decode or encode UTF-8 |
| `char::from_u32(n)` | a helper that checks the range and the surrogates, then boxes the value |

- **Locals of heap types** are non-nullable Wasm locals, except options, which are nullable. This is valid in Wasm because every WaSL local of a non-nullable type is set by its declaration (§ 9.4) before any use in the same block, and a match binding is set at the start of its case block.
- **Long string literals**: Wasm implementations limit the number of operands of `array.new_fixed`. A literal longer than 1,000 bytes is placed in a passive data segment and built with `array.new_data $bytes` each time it is evaluated, which § 10.9 allows. `array.new_data` is not a constant expression, so in the initial value of a global (§ 8.18) a literal always uses `array.new_fixed`, whatever its length; Wasm runtimes accept it, but compiling a very long one is slow.
- **Helpers** (string comparison, concatenation, UTF-8 validation, the boundary adapters of chapter 13) are ordinary Wasm functions, included only in the modules that use them (§ 12.1).

## 12.9 Integer overflow

This section gives the checks of § 10.2 and § 10.5 used by § 12.2 to § 12.4. Each check computes the wrapped result with the instruction of § 12.3, then tests whether the exact result fits; if not, it executes `i32.const 0x80000000` `i32.const -1` `i32.div_s`, as for `MIN / -1` on `s8` (§ 12.2), so that every overflow traps with the `integer overflow` of § 10.7. The test is an inline branch that is not taken when nothing overflows.

| Operation | Overflow test |
|---|---|
| `+` `-` `*` unary `-` on 8- and 16-bit integers | the operands are normalized `i32`s (§ 12.2), so the exact result fits in an `i32`; it overflowed when it is out of the range of the type: `i32.gt_u` with `0xFF` or `0xFFFF` for `u8` and `u16`, `i32.extend8_s` or `i32.extend16_s` then `i32.ne` for `s8` and `s16`. This test replaces the normalization. |
| `+` on `u32`, `u64` | `r = a + b` overflowed when `r < b` (`lt_u`) |
| `-` on `u32`, `u64` | overflows when `a < b` (`lt_u`) |
| `+` on `s32`, `s64` | `r = a + b` overflowed when `(a ^ r) & (b ^ r) < 0` |
| `-` on `s32`, `s64` | `r = a - b` overflowed when `(a ^ b) & (a ^ r) < 0` |
| unary `-` on `s32`, `s64` | overflows when the operand is the minimum of the type |
| `*` on `u32`, `s32` | the operands extended to `i64` (`extend_i32_u` or `extend_i32_s`), multiplied with `i64.mul`; it overflowed when the product is not the extension of its low 32 bits |
| `*` on `u64`, `s64` | when both operands fit in 32 bits (signed or unsigned as the type), the product cannot overflow, which is tested inline; otherwise a helper tests `r / a != b` for `a != 0`, the signed case `a == -1` being tested apart |
| integer conversion that may not fit (§ 12.4) | before the conversion, the value is compared with the bounds of the target type, in the width of the source: `lt_s` and `gt_s` when both types are signed, otherwise one `gt_u` with the maximum, which also catches negative values |
| float to integer (§ 12.4) | the trapping `trunc` instructions, which trap on NaN and out of range: `i32.trunc_f64_s`, `i64.trunc_f32_u`… For 8- and 16-bit targets, `i32.trunc_f32_s` or `i32.trunc_f64_s`, then the conversion test above from `s32`. |

The built-in functions of § 8.17 use the same code without the trap: `T::wrapping_add`, `wrapping_sub`, `wrapping_mul`, `wrapping_neg` and `wrapping_from` are the instructions and the normalization of § 12.2 to § 12.4, without the test; `T::checked_add`, `checked_sub` and `checked_mul` compute the same test and build `option::none` or `option::some` of the result instead of trapping.

## 12.10 Resources and handles

**Resources of the package.** In a world that does not import its interface, a resource `r` is a struct, built like a record from its fields in declaration order, with `mut` fields as `(mut T')`:

| WaSL | Wasm |
|---|---|
| resource literal `r { … }` | the values, `struct.new $r` |
| constructor `r(a)` | `a` `call $r.constructor`, a function whose body returns the new struct |
| method `e.m(a)` | `e` `a` `call $r.m`; `self` is the first parameter, `(ref $r)` |
| static function `r::s(a)` | `a` `call $r.s` |
| `self.f`, `self.f = v;` | `struct.get $r f`, `struct.set $r f` |

The table of handles that other components hold on these objects is part of the boundary (§ 13.11).

**Handles.** In a world that imports the interface of a resource, or for a resource of another package, a value of the resource is an `i32` handle:

| WaSL | Wasm |
|---|---|
| constructor, method, static function | a call to the imported function of § 13.10; `self` is the handle, lent as a `borrow` |
| move of `x` (§ 9.9) | `local.get $x` `i32.const 0` `local.set $x` |
| release of `x` | `local.get $x` `if` (`x != 0`) `local.get $x` `call $r_drop` `end`, then `i32.const 0` `local.set $x`, so that a local declared in a loop is never released twice |
| temporary lent to a call | `local.tee` of the argument into a fresh local, then its release after the call |
| `match` case without a name, `else` | the value of the scrutinee given to the helper below |
| assignment `x = e;` | `e` into a fresh local, the release of `x`, then `local.set $x` |
| release of a temporary given to `_ =` | a generated helper per type that walks the value and calls `$[resource-drop]r` on every handle |

The adapter of an exported function (§ 13.8) keeps the handles lent to it, `borrow<r>`, in fresh locals and releases them after the call, as the Component Model requires before the function returns.

`option<r>`, where `r` is a handle type, boxes the `i32` (§ 12.7); variants and results hold it as an `i32` payload. Releases are emitted at each exit of a block, before its `br` or `return`, in the reverse order of the declarations (§ 10.11).

## 12.11 Error propagation

`e?` compiles to the instructions of the equivalent `match` (§ 8.19), without the `br_table`:

| Operand | Wasm |
|---|---|
| `result<T, E>` | `e` into a local; `struct.get $v 0` `if` (tag `1`) the releases of § 12.10, the `err` of the function's `result<U, E>`, built from the payload (`ref.cast` `struct.get $v.err 1` `struct.new $u.err`) or `global.get` of its shared instance without payload, `return` `end`; then `ref.cast (ref $v.ok)` `struct.get $v.ok 1` for the payload |
| `option<T>` | `e` `br_on_non_null $l`, the releases, `ref.null` of the function's option, `return`; at `$l`, the reference itself or `struct.get $box_T 0` |

The `err` value is rebuilt because `result<T, E>` and `result<U, E>` are two distinct struct families (§ 12.6), even with the same `E`; its payload is not copied, only its reference.
