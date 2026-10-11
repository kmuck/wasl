# v2.1 Overview

## v2.1.1 Scope of v0.2

**In v0.2**, in addition to v0.1:

- the types `string`, `list<T>`, `option<T>` and `result<T, E>`, which have no declaration (§ v2.2.2);
- the declarations `record`, `enum` and `variant`, written inside interfaces like in WIT (§ v2.2.3);
- string literals, record literals, list literals and constructors (§ v2.4);
- field access `r.f`, indexing `xs[i]`, and assignment to `mut` fields and to elements of `list<mut T>` (§ v2.4);
- the `match` statement on enums, variants, options and results (§ v2.4.9);
- a small set of built-in functions on strings and lists: `string::len`, `string::bytes`, `string::from_bytes`, `list::len`, `list::filled`, `list::copy` (§ v2.4.11);
- globals of these types, initialized with constant expressions (§ v2.4.12);
- exported functions that take and return these types, through the Canonical ABI with linear memory (chapter v2.7).

**Still not in v0.2** (chapter 16 and § v2.11): `char`, `tuple`, `flags`, type aliases (`type`), `use` in source code, imports, WASI, resources, async, an error propagation operator, string formatting.

## v2.1.2 Design decisions

Each decision follows from the principles of § 1.2. The rest of this document details them.

1. **The types are the types of WIT.** Each new type has the name, the cases and the meaning of the WIT type of the same name; the WIT of a package is still derived by erasure (§ 2.6). Types are declared **inside interfaces**, as in WIT, and named from another interface with a path, `invoices::line`.
2. **Values live on the WasmGC heap.** A record is a WasmGC `struct`, a list a WasmGC `array`, a string an `array` of UTF-8 bytes. The component embeds no allocator and no collector: the runtime provides both (§ 1.1). Enums are plain integers, as in the Canonical ABI.
3. **Reference semantics inside a component.** Assigning a record, a list or a string, or passing it to a function, copies a reference, as for objects in Java or JavaScript. Nothing is copied implicitly inside a component.
4. **Copies at the boundary only.** A value that crosses the boundary of a component is deep-copied through linear memory, as the Canonical ABI requires today. Linear memory is used **only** as a scratch buffer for these copies, managed by a bump allocator that is reset after every call (§ v2.7.4). When runtimes support the `gc` option of the Canonical ABI, these copies disappear without changing the language (§ v2.11).
5. **Mutability where WasmGC declares it.** A record field is immutable unless declared `mut`; the elements of `list<T>` are read-only and those of `list<mut T>` are assignable (§ 1.2, principle 2). Strings are immutable.
6. **No null, no default value.** Every value of a heap type is a valid value: there is no `null`, and the absence of a value is written `option<T>`. A local of a heap type therefore needs an initial value (§ v2.4.13).
7. **One way to write each thing.** A record literal lists its fields in declaration order; every constructor is written with its type (`option::some(x)`, `status::paid`); a variant is inspected only with `match`.

## v2.1.3 Changes for v0.1 programs

v0.2 accepts every valid v0.1 program with the same behavior, with these exceptions:

| Change | Effect on v0.1 programs |
|---|---|
| `match` and `case` become keywords (§ v2.3.2) | an identifier named `match` or `case` is now E0005 |
| the initial value of a global is a constant expression checked by the typing rules (§ v2.4.12) | `global g: u32 = 1 + 1;` reports E0409 instead of E0100 |
| a lone `_` is a token (§ v2.3.1) | `local _: u32;` reports E0100 instead of E0004 |
| exported functions have no parameter limit (§ v2.7.3) | E0205 is no longer reported |
| `weasel call` prints NaN as `nan` (§ v2.9) | output of `weasel call` for a NaN float |

A component whose exported functions only use scalar types is built exactly as in v0.1: it has no linear memory and no `realloc` (§ v2.7.3).
