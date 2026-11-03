# v2.5 Runtime semantics

This chapter extends chapter 10.

## v2.5.1 References and sharing

A value of a heap type is a **reference** to an object of the WasmGC heap. Inside a component:

- assigning it, passing it as an argument, returning it, storing it in a field, a list element or a payload copies the reference, never the object;
- two names that hold the same reference see the same object: a change made through one is visible through the other.

```wasl
record counter { hits: mut u32 }

local a: counter = counter { hits: 0 };
local b: counter = a;
b.hits = 1;                   // a.hits is now 1 as well: a and b are the same record
```

Sharing is only observable through what can change: `mut` fields and the elements of `list<mut T>`. Strings, tuples and `list<T>` never change, so whether two of them are the same object cannot be observed; the language has no identity comparison (§ v2.11).

`list::filled(n, x)` shares `x` between all the elements: with a record `x` that has `mut` fields, changing the field through one element changes it for all of them.

**Lifetime.** An object lives as long as it is reachable. The runtime's garbage collector reclaims it afterwards; a WaSL program never frees memory and cannot observe when an object is reclaimed.

**Chars, enums and flags** are values, not references: they are copied like numbers.

## v2.5.2 Evaluation order

In addition to § 10.1, always from left to right:

- the fields of a record literal, in declaration order (which is the written order, § v2.4.4), then the record is created;
- the elements of a list literal or of `tuple(…)`, then the list or tuple is created;
- the payload of a constructor, then the value is created;
- in `e.f`, `e`; in `e[i]`, `e` then `i`, then the bounds are checked;
- in an assignment `p = v;`, the object and index designated by `p` first (from left to right), then `v`, then the bounds are checked and the value is stored;
- the operands of `+` on strings, then the new string is created;
- in `match (e)`, `e` once, then the block of its case runs.

## v2.5.3 Creation

Each evaluation of a record literal, a non-empty list literal, a tuple, a constructor with a payload, `+` on strings, `list::filled`, `list::copy`, `list::freeze`, `string::chars`, `string::from_chars` and `string::from_bytes` (when it returns `some`) creates a **new** object. Two evaluations of the same literal create two distinct objects.

String literals, constructors without payload and empty lists may be shared by the implementation: nothing can modify them, so the difference cannot be observed.

## v2.5.4 Values at the component boundary

When an exported function is called, its arguments come from **outside** the component (the host or another component), and its result goes outside. Today these values are **deep copies** (chapter v2.7), and sharing is not preserved: a record that appears twice in a returned list arrives as two distinct, equal records.

Calls between functions of the same component never copy, including calls to functions of exported interfaces: `total(inv)` inside `mark_paid` receives the same record.

The difference cannot be observed. The values that cross the boundary contain nothing that can change (§ v2.2.7), and the language has no identity comparison: a function cannot tell a copy from a shared reference. An exported function therefore behaves the same whether it is called from outside or from inside the component, and will still do so when the `gc` option of the Canonical ABI removes the copies (§ v2.11).

## v2.5.5 Traps

New causes of traps, in addition to § 10.7:

| Cause | Message reported by wasmtime |
|---|---|
| integer arithmetic or conversion whose exact result does not fit in its type (§ v2.5.6) | `integer overflow` |
| conversion of a NaN float to an integer (§ v2.5.6) | `invalid conversion to integer` |
| index outside a list: `xs[i]` or `xs[i] = e` with `i >= list::len(xs)` | `out of bounds array access` |
| object too large: `list::filled(n, x)`, `+` or `list::copy` above the size limit of the runtime | `allocation size too large` |
| heap exhausted | the runtime's out-of-memory message |

Nothing else traps: there is no null reference, no failing cast, and no invalid UTF-8 (`string::from_bytes` returns `option::none`). These causes replace the last sentence of § 10.7: overflow no longer wraps, and conversions can trap. Messages are those of the reference runtime; their exact text is not part of the language.

A trap during an exported call discards the whole instance (§ 10.7), with its heap.

## v2.5.6 Integer arithmetic and conversions

This section replaces the wrapping arithmetic of § 10.2 and the integer results of § 10.5. **An integer operation gives its exact result, or traps**: a wrong number never continues silently.

**Arithmetic.** The result of `+`, `-`, `*` and unary `-` on an integer type is the exact mathematical result. If it does not fit in the type, the call traps with `integer overflow`.

| Type | Expression | Result |
|---|---|---|
| `u8` | `200 + 55` | `255` |
| `u8` | `255 + 1` | trap |
| `u32` | `0 - 1` | trap |
| `s8` | `-(-128)` | trap |
| `u16` | `300 * 300` | trap |
| `s64` | `9223372036854775807 + 1` | trap |

Division and remainder are unchanged (§ 10.2): dividing by zero and `MIN / -1` already trap, `MIN % -1` is `0`. The bitwise operators and shifts work on bits, not on numbers, and never trap: `<<` still loses the bits shifted out of the type. Masking is therefore the explicit way to keep low-order bits: `u8(x & 255)` never traps.

**Conversions between integer types.** `T(e)` is the value of `e` if it is a value of `T`; otherwise the call traps with `integer overflow`. A conversion to a type that contains every value of the source type never traps.

| Conversion | Result |
|---|---|
| `u8(n)` with `n: u32` = 255 | `255` |
| `u8(n)` with `n: u32` = 300 | trap |
| `u32(n)` with `n: s32` = -1 | trap |
| `s8(n)` with `n: u8` = 200 | trap |
| `u64(n)` with `n: s8` = -1 | trap |
| `s64(n)` with `n: u32` | never traps |

**Conversions from a float to an integer.** The value is truncated toward zero, as in § 10.5, and the conversion traps if the truncated value does not fit in the type (`integer overflow`) or if the float is NaN (`invalid conversion to integer`). With `x: f64`, `u8(x)` is `3` for 3.9 and `0` for -0.9, and traps for -1.0, 300.7 and NaN. There is no more saturation.

The other conversions of § 10.5 are unchanged: integer to float gives the nearest float, `f64` to `f32` the nearest `f32`.

**When wrapping is wanted.** Hashes, checksums and random number generators compute modulo 2ⁿ on purpose. They say so with the built-in functions of § v2.4.11: `u32::wrapping_add(h, c)` gives the result of § 10.2, `u8::wrapping_from(n)` the low-order bits of § 10.5. A program that treats an overflow as an error rather than a trap uses `u64::checked_mul(a, b)`, which gives `option::none` instead of trapping.

**Why.** An exported call that traps fails alone: the host gets an error, and the instance is discarded with its state (§ 10.7). A wrong number, on the contrary, continues its way into a result, a stored value or another component. For a service that answers each request with a fresh instance, the trap is the failure that costs the least. WebAssembly already makes this choice for division (`div_s` traps on `MIN / -1`) and for its `trunc` conversions; the plain wrapping instructions remain available through the `wrapping_` functions.

