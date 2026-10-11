# v2.6 Mapping to WasmGC

This chapter extends chapter 12: how the new types are represented in the core module of a world, and which instructions implement them. As in chapter 12, the instructions given are those of the reference implementation; any other code with the same behavior conforms.

The core module of a world that uses heap types requires the WebAssembly **GC** proposal, with the reference types and typed function references it builds on, all part of WebAssembly 3.0. `weasel` enables them in wasmtime.

## v2.6.1 Representation of the types

`T'` is the Wasm type of a WaSL type `T`:

| WaSL type | `T'` | Type definition |
|---|---|---|
| v0.1 types | as in § 7.1 | |
| `char` | `i32` | the code point |
| enum | `i32` | the index of the case, in declaration order from 0 |
| flags | `i32` | bit `k` set when the flag `k` (in declaration order, from 0) is in the set |
| `string` | `(ref $bytes)` | `$bytes = (array (mut i8))` |
| `list<T>`, `list<mut T>` | `(ref $list_T)` | `$list_T = (array (mut E))`, with `E` below |
| `tuple<T1, …, Tn>` | `(ref $tuple)` | `$tuple = (struct (field T1') … (field Tn'))`, immutable fields |
| record `r` | `(ref $r)` | `$r = (struct (field F1) …)`, one field per WaSL field, in declaration order; a `mut` field is `(mut T')` |
| variant `v` | `(ref $v)` | see § v2.6.2 |
| `result<…>` | `(ref $v)` | a variant with the cases `ok` (0) and `err` (1) |
| `option<T>` | `(ref null X)` | see § v2.6.3 |

**List elements.** `E` is `i8` for `bool`, `s8` and `u8`; `i16` for `s16` and `u16`; `T'` otherwise. Packed elements are read with `array.get_s` for signed types and `array.get_u` otherwise, which gives the normalized `i32` of § 12.2.

**Same representation, distinct types.** `string`, `list<u8>`, `list<s8>` and `list<bool>` share `$bytes`; `list<T>` and `list<mut T>` share `$list_T`.

**Why immutable lists are mutable Wasm arrays today.** WasmGC can only create an immutable array (`(array T)`) with `array.new` (one value repeated), `array.new_fixed` (a number of values fixed in the code) or `array.new_data` / `array.new_elem` (the contents of a segment of the binary). Filling an array element by element, with `array.set` or `array.copy`, requires a mutable array, and a mutable array cannot be turned into an immutable one: `(array (mut T))` is not a subtype of `(array T)`. A list received at the component boundary, the result of `list::freeze` or of `string::chars`, has contents known only at run time, so it can only be built in a mutable array. Every list is therefore an `(array (mut E))`, and the immutability of `list<T>` (§ v2.2.6) is enforced by the type checker: no code that writes to a `list<T>` is ever generated. When WasmGC can build an immutable array from values computed at run time, `list<T>` becomes an `(array E)` without any change to the language. Strings follow the same rule and are never written after they are built; this is why `string::bytes` costs nothing.

**Record fields** use the representation of § 7.1 for scalars (an `i32` for a `u8` field), not packed fields: struct fields are few, and normalized `i32`s need no extension on reads.

**Type identity.** WasmGC types are structural: two WaSL records with the same field types may become the same Wasm type. Generated code never relies on a cast to tell two WaSL types apart; the type checker already did. Variant cases are told apart by their tag (§ v2.6.2), never by a cast alone.

**Recursion group.** All the GC types of the module (structs and arrays) are defined in **one** recursion group (`rec`), placed before the function types, so that types that refer to each other in a cycle (§ v2.2.5) need nothing special. Within a module this behaves exactly like one group per cycle: GC types never cross the boundary (chapter v2.7), so their canonical identity across modules does not matter.

## v2.6.2 Variants and results

A variant `v` is a small family of struct types:

```wasm
(type $v      (sub (struct (field i32))))                             ;; the tag: index of the case
(type $v.c    (sub final $v (struct (field i32) (field P'))))         ;; one per case with a payload P
```

- A value is a struct whose first field, the **tag**, is the index of its case in declaration order. A case with a payload is a `$v.c`; a case without payload is a `$v` with only the tag.
- A case without payload is created once per instance, in an immutable global initialized with `struct.new $v (i32.const k)`, and reused.
- `match` reads the tag with `struct.get $v 0` and dispatches with `br_table`. In the block of a case with a binding, `ref.cast (ref $v.c)` then `struct.get $v.c 1` give the payload; the cast never fails, since the tag was checked.

`result<T, E>` uses the same layout, with `ok` = 0 and `err` = 1; a form without a payload for a case (§ v2.2.2) has no payload field for that case.

## v2.6.3 Options

`option<T>` uses a nullable reference, `none` being `null`:

- if `T'` is a non-nullable reference `(ref $X)` (a string, a list, a record, a variant, a result), `option<T>` is `(ref null $X)`: `option::some(x)` is `x` itself, with no allocation;
- otherwise (`T` is a scalar, or itself an option, which is already nullable), the payload is boxed: `$box_T = (struct (field T'))`, `option<T>` is `(ref null $box_T)`, and `option::some(x)` is `struct.new $box_T`.

`match` on an option tests with `br_on_null`; the payload is the reference itself, or `struct.get $box_T 0`.

## v2.6.4 Instructions

| WaSL | Wasm |
|---|---|
| `local x: T = e;` (heap type) | a Wasm local of type `T'`; `e` then `local.set` |
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

- **Locals of heap types** are non-nullable Wasm locals. This is valid in Wasm because every WaSL local of a heap type is set by its declaration (§ v2.4.13) before any use in the same block, and a match binding is set at the start of its case block.
- **Long string literals**: Wasm implementations limit the number of operands of `array.new_fixed`. A literal longer than 1,000 bytes is placed in a passive data segment and built with `array.new_data $bytes` each time it is evaluated, which § v2.5.3 allows. `array.new_data` is not a constant expression, so in the initial value of a global (§ v2.4.12) a literal always uses `array.new_fixed`, whatever its length; Wasm runtimes accept it, but compiling a very long one is slow.
- **Helpers** (string comparison, concatenation, UTF-8 validation, the boundary adapters of chapter v2.7) are ordinary Wasm functions, included only in the modules that use them (§ 12.1).

## v2.6.5 Content of the module

The list of § 12.1 is extended:

- **types**: the type definitions of § v2.6.1 used by the included functions and globals, then the function types;
- **globals**: in addition to v0.1 globals, one immutable global per distinct string literal and per shared case without payload, then the globals of § v2.7.4 when the module has a memory;
- **functions**: the included functions, the helpers they use, and the boundary adapters of the exported functions (§ v2.7.5);
- **memory**, **realloc** and **post-return** functions only when an exported function needs them (§ v2.7.3).
