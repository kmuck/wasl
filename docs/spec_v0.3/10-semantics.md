# 10. Runtime semantics

This chapter defines what a valid program computes. Most rules are those of WebAssembly itself (§ 1.2, principle 1); integer overflow is the exception (§ 1.3, decision 8).

## 10.1 Evaluation order

- The operands of a binary operator are evaluated from left to right.
- The arguments of a call are evaluated from left to right, then the function is called.
- `&&` and `||` short-circuit: the right operand is only evaluated if the left one does not decide the result (`false && …` and `true || …` do not evaluate their right operand).
- In an assignment `p = v;`, the object and index designated by `p` first (from left to right), then `v`; then the bounds are checked and the value is stored.
- The fields of a record literal are evaluated in declaration order (which is the written order, § 8.11), then the record is created.
- The elements of a list literal or of `tuple(…)` are evaluated, then the list or tuple is created; the payload of a constructor, then the value is created.
- In `e.f`, `e` is evaluated; in `e[i]`, `e` then `i`, then the bounds are checked.
- The operands of `+` on strings are evaluated, then the new string is created.
- In `match (e)`, `e` is evaluated once, then the block of its case runs.

## 10.2 Integers

**Exact arithmetic.** The result of `+`, `-`, `*` and unary `-` on an integer type is the exact mathematical result. If it does not fit in the type, the call traps with `integer overflow`: a wrong number never continues silently.

| Type | Expression | Result |
|---|---|---|
| `u8` | `200 + 55` | `255` |
| `u8` | `255 + 1` | trap |
| `u32` | `0 - 1` | trap |
| `s8` | `-(-128)` | trap |
| `u16` | `300 * 300` | trap |
| `s64` | `9223372036854775807 + 1` | trap |

**Division `/`.** Truncated toward zero: `-7 / 2 = -3`, `7 / -2 = -3`.

- Division by zero: **trap**.
- For a signed type, `MIN / -1` (whose exact result does not fit in the type, e.g. `-128 / -1` in `s8`): **trap**, for every signed type, including `s8` and `s16`.

**Remainder `%`.** The result has the sign of the dividend: `-7 % 2 = -1`, `7 % -2 = 1`.

- Remainder by zero: **trap**.
- `MIN % -1 = 0` (no trap).

**Bitwise operators** `&`, `|`, `^` operate on the bits of the value. They and the shifts work on bits, not on numbers, and never trap. Masking is therefore the explicit way to keep low-order bits: `u8(x & 255)` never traps.

**Shifts.**

- `<<` shifts left, inserting zeros.
- `>>` is an arithmetic shift for a signed type (the sign bit is copied: `-128 >> 1 = -64` in `s8`) and a logical shift for an unsigned type (zeros are inserted).
- The shift amount is the right operand, interpreted as unsigned and **taken modulo the bit width of the type**: for a `u8`, `x << 9` is `x << 1`; for a `u32`, `x << 33` is `x << 1`. The bits shifted out of the type are lost (`u8`: `129 << 1 = 2`).

**Comparisons** use the signed order for signed types and the unsigned order for unsigned types: with `u32`, `1 < 4294967295` is `true`; with `s32`, `1 < -1` is `false`.

**When wrapping is wanted.** Hashes, checksums and random number generators compute modulo 2ⁿ on purpose. They say so with the built-in functions of § 8.17: `u32::wrapping_add(h, c)` gives the result reduced modulo 2ⁿ, where n is the bit width of the type (`u8::wrapping_add(255, 1)` is `0`, `s8::wrapping_neg(-128)` is `-128`), and `u8::wrapping_from(n)` the low-order bits of `n`. A program that treats an overflow as an error rather than a trap uses `u64::checked_mul(a, b)`, which gives `option::none` instead of trapping.

**Why.** An exported call that traps fails alone: the host gets an error, and the instance is discarded with its state (§ 10.7). A wrong number, on the contrary, continues its way into a result, a stored value or another component. For a service that answers each request with a fresh instance, the trap is the failure that costs the least. WebAssembly already makes this choice for division (`div_s` traps on `MIN / -1`) and for its `trunc` conversions; the plain wrapping instructions remain available through the `wrapping_` functions.

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
| integer → integer | the value of `e`, if it is a value of `T`; otherwise the call **traps** with `integer overflow`. A conversion to a type that contains every value of `S` never traps. | `u32` 255 → `u8` 255; `u32` 300 → `u8` trap; `s32` −1 → `u32` trap; `u8` 200 → `s8` trap; `s8` −1 → `u64` trap; `u32` → `s64` never traps |
| integer → float | the nearest float value | `u64` 18446744073709551615 → `f64` 18446744073709551616.0; `s16` −5 → `f64` −5.0 |
| float → integer | truncation toward zero; the call **traps** if the truncated value does not fit in `T` (`integer overflow`) or if the float is NaN (`invalid conversion to integer`) | `f64` −3.9 → `s32` −3; `f64` 3.9 → `u8` 3; `f64` −0.9 → `u8` 0; `f64` −1.0 → `u8` trap; `f64` 300.7 → `u8` trap; `f64` NaN → `u32` trap |
| `f64` → `f32` | the nearest `f32` value; ± infinity if the value is out of range | `f64` 0.1 → the `f32` nearest to 0.1 |
| `f32` → `f64` | the exact same value | |
| `char` → `u32` | the code point of the char | `'A'` → 65 |

The low-order bits of an integer, read as a `T`, are written `T::wrapping_from(e)` (§ 8.17): `u8::wrapping_from(n)` is `44` for `n: u32` = 300, `s8::wrapping_from(n)` is `−56` for `n: u8` = 200.

## 10.6 Calls and globals

- Calling a function evaluates its body with its parameters bound to the arguments. Recursion is allowed.
- Globals keep their value for the lifetime of the instance of the component (§ 6.4). Each call to an exported function on the same instance sees the changes made by the previous calls.

## 10.7 Traps

A **trap** stops the current call immediately. The host receives an error and the instance of the component must not be used again: it is discarded with its heap. The causes of a trap are:

| Cause | Message reported by wasmtime |
|---|---|
| integer division or remainder by zero | `integer divide by zero` |
| signed `MIN / -1` | `integer overflow` |
| integer arithmetic or conversion whose exact result does not fit in its type (§ 10.2, § 10.5) | `integer overflow` |
| conversion of a NaN float to an integer (§ 10.5) | `invalid conversion to integer` |
| index outside a list: `xs[i]` or `xs[i] = e` with `i >= list::len(xs)` | `out of bounds array access` |
| object too large: `list::filled(n, x)`, `+` or `list::copy` above the size limit of the runtime | `allocation size too large` |
| heap exhausted | the runtime's out-of-memory message |
| call stack exhaustion (recursion too deep) | `call stack exhausted` |
| use of a moved handle: a method call or an argument with the empty handle `0` (§ 9.9) | `unknown handle index 0` |
| a trap in an imported function, in the component or host that provides it | the message of that trap |

Nothing else traps: float operations never trap, there is no null reference (the empty handle is not a reference), no failing cast, and no invalid UTF-8 (`string::from_bytes` returns `option::none`). Messages are those of the reference runtime; their exact text is not part of the language.

## 10.8 References and sharing

A value of a heap type is a **reference** to an object of the WasmGC heap. Inside a component:

- assigning it, passing it as an argument, returning it, storing it in a field, a list element or a payload copies the reference, never the object;
- two names that hold the same reference see the same object: a change made through one is visible through the other.

```wasl
record counter { hits: mut u32 }

local a: counter = counter { hits: 0 };
local b: counter = a;
b.hits = 1;                   // a.hits is now 1 as well: a and b are the same record
```

Sharing is only observable through what can change: `mut` fields and the elements of `list<mut T>`. Strings, tuples and `list<T>` never change, so whether two of them are the same object cannot be observed; the language has no identity comparison (§ 16).

`list::filled(n, x)` shares `x` between all the elements: with a record `x` that has `mut` fields, changing the field through one element changes it for all of them.

**Lifetime.** An object lives as long as it is reachable. The runtime's garbage collector reclaims it afterwards; a WaSL program never frees memory and cannot observe when an object is reclaimed.

**Chars, enums and flags** are values, not references: they are copied like numbers.

## 10.9 Creation

Each evaluation of a record literal, a non-empty list literal, a tuple, a constructor with a payload, `+` on strings, `list::filled`, `list::copy`, `list::freeze`, `string::chars`, `string::from_chars` and `string::from_bytes` (when it returns `some`) creates a **new** object. Two evaluations of the same literal create two distinct objects.

String literals, constructors without payload and empty lists may be shared by the implementation: nothing can modify them, so the difference cannot be observed.

## 10.10 Values at the component boundary

When an exported function is called, its arguments come from **outside** the component (the host or another component), and its result goes outside. Today these values are **deep copies** (chapter 13), and sharing is not preserved: a record that appears twice in a returned list arrives as two distinct, equal records.

Calls between functions of the same component never copy, including calls to functions of exported interfaces: `total(inv)` inside `mark_paid` receives the same record.

The difference cannot be observed. The values that cross the boundary contain nothing that can change (§ 7.11), and the language has no identity comparison: a function cannot tell a copy from a shared reference. An exported function therefore behaves the same whether it is called from outside or from inside the component, and will still do so when the `gc` option of the Canonical ABI removes the copies (§ 16).

## 10.11 Resources and imports (specified, not available yet)

**Resources of the package.** An object of a resource is a heap object like a record (§ 10.8): assigning it or passing it copies the reference, and a change of a `mut` field through one name is visible through every other. Each evaluation of a constructor creates a new object.

**Resources at the boundary are not copied.** When an object of a resource of the package leaves the component, the other side receives a handle to **the same object**: its later method calls see and change the state the component sees. When the handle comes back, as an argument, the component gets the same object back. This is the only kind of value whose identity crosses the boundary, and it is the Component Model that guarantees it. The object stays alive while the other side holds a handle to it, even if the component no longer references it; when the other side releases its last handle, the component forgets it and the runtime reclaims it like any object (§ 12.10).

**Handles.** A handle of an imported resource is released at the points of § 9.9, in this order at the end of a block: the locals in the reverse order of their declarations. Releasing a handle runs the destructor of the resource in the component that owns it, which is the only way a WaSL program runs code in another component without a call written in the source.

**Calls to imported functions.** A call to a function of an imported interface evaluates its arguments from left to right, then transfers control to the component or the host that provides it (§ 13.10). Its arguments are copied (§ 10.10), except handles, which are moved or lent (§ 9.9). It cannot call the component back: a component that calls an import is not re-entered before the call returns, which the Component Model guarantees for synchronous calls.

**State of an imported interface of the package.** An interface of the package imported by a world (§ 2.5) runs in another instance, with its own globals: the importing component does not share the globals of that instance, even though the source code is the same.
