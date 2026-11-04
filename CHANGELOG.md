# Changelog

All notable changes to WaSL are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Versioning

- The **language** is versioned by its specification: `docs/spec_v0.1`, `docs/spec_v0.2`…
- The two crates, [`waslc`](waslc) and [`weasel`](weasel), follow [Semantic Versioning](https://semver.org/) and are always released together with the **same version number**.
- While WaSL is in `0.x`, the minor version of the crates is the version of the specification they implement: `0.2.x` implements the v0.2 specification. Patch releases (`0.2.1`, `0.2.2`…) only fix bugs and never change the language.
- Each release is a git tag `vX.Y.Z` on `main`, with a matching GitHub Release.

## [Unreleased]

### Added

- Imports (§ 2.5, § 13.10): a world imports an interface of a dependency, `import wasi:cli/stdout;`, or of the package, `import accounts;`. Code calls its functions and uses its types with the last part of its name as path (`log::write(log::entry { … })`), and the generated WIT writes `import …@version;` and `use …@version.{…}`. Calls go through reverse adapters that lower the arguments and lift the result through the Canonical ABI. An interface of the package imported by a world is not compiled into its component. Resources and `async` functions of other packages are not available yet. New errors E0215 (unknown package or interface, or an item WaSL cannot use), E0216 (interface imported twice, or imported and exported), E0217 (two interfaces with the same name in code) and E0221 (call to an interface of another package that the world does not import). `export` of an interface of another package is a syntax error until foreign exports are implemented. `import` is now a keyword.
- `weasel call` picks a world without imports that exports the function, and says so when only worlds with imports export it: providing imports comes with composition.
- Dependencies (§ 3.5): `wasl.toml` accepts a `[dependencies]` table. A dependency is an exact version fetched from a registry with the crates of `wkg` and pinned in `wkg.lock`, or a `path` to a WaSL project or a directory of WIT files. Every `weasel` command resolves them first; invalid entries, mismatched package names, conflicting versions and cycles are E0200. `waslc::analyze` takes the WIT of the dependencies as a new argument, `waslc::Dependencies`.
- The `?` operator on `result` and `option` (§ 8.19): `local p: u64 = prices::of(sku)?;` returns the `err` or the `none` from the function. `f(…)?;` is a statement when the `ok` has no payload. New errors E0414 (operand that is not a `result` or an `option`) and E0415 (return type that cannot receive the error). `?` is now a token, so a misplaced `?` is a syntax error (E0100) instead of E0001.
- The v0.3 specification describes the features of 0.3, marked *specified, not available yet* until the compiler implements them: dependencies in `wasl.toml` and `wkg.lock` (§ 3.5), imports of interfaces of other packages and of the same package (§ 2.5), implementations of foreign interfaces such as `wasi:cli/run` (§ 2.4.2, § 13.12), resources with constructors, methods and static functions (§ 7.12, § 8.20), automatic release of handles (§ 9.9), the `?` operator (§ 8.19), and the errors E0215 to E0221, E0308, E0309, E0414 and E0415 (§ 11.3).
- The target programs of 0.3, written before the compiler supports them: [`examples/catalog`](examples/catalog) and [`examples/checkout`](examples/checkout), two components linked by a resource, and [`examples/hello`](examples/hello), a WASI 0.2 command.

### Changed

- The v0.2 specification is frozen. The [v0.3 specification](docs/spec_v0.3/README.md) merges v0.1 and v0.2 into one complete document, without changing any rule; it will receive the features of 0.3. Its README maps the old `§ v2.x.y` numbers to the new ones, and the comments of the compiler cite the new numbers.
- `ROADMAP.md`: the scope of 0.3, with its two target programs (`catalog` and `checkout` composed, and a WASI 0.2 `hello`).

## [0.2.0] - 2026-11-04

Data on the WasmGC heap, implementing the [v0.2 specification](docs/spec_v0.2/README.md) (provisional until v0.3). The target program, [`examples/invoice`](examples/invoice), builds and runs in the continuous integration.

### Added

- `ROADMAP.md`: the direction of WaSL from 0.2 to 1.0, with the target program of each version.
- `waslc` parses the whole v0.2 syntax: string and char literals, type declarations in interfaces, composite types, `match`, field accesses, indexes, record, flags, list and tuple literals.
- `record`, `enum`, `variant` and `flags` declarations in interfaces, and the built-in types `char`, `string`, `list`, `tuple`, `option` and `result` in signatures and declarations, with their errors (E0206 to E0212, E0305).
- The WIT of a package contains the types of its interfaces, and types-only interfaces used through `use`.
- Typing of the v0.2 expressions and statements: string, char, record, flags, list and tuple literals, constructors, field accesses, indexes, `match`, the operators on chars, strings, enums and flags, the built-in functions, constant globals and assignments to `mut` fields and `list<mut T>` elements, with their errors (E0306, E0307, E0406 to E0412, E0505 to E0507). `examples/invoice` passes semantic analysis and `weasel wit` prints its WIT.
- E0211 rejects only the recursive types that have no value (§ v2.2.5): a variant has a value as soon as one of its cases has one, so `variant expr { lit(u32), neg(expr) }` is accepted, and `result<a>` has the value `err`. The message names the field without a value and the possible fixes.
- E0213: interfaces of the WIT that use each other's types in a cycle, which WIT forbids, are reported with the steps of the cycle.
- E0214: a `mut` field or a `list<mut T>` in a type that appears in the WIT (the signature of an exported function, or a type of an exported interface) is an error (§ v2.2.7). Mutability stays inside the component, where it maps to WasmGC.
- `_ = f(…);` discards the value of a call explicitly (§ v2.4.16); it is the `drop` that Wasm requires.
- Built-in functions on each integer type `T` (§ v2.4.11): `T::wrapping_add`, `T::wrapping_sub`, `T::wrapping_mul`, `T::wrapping_neg` (signed types) and `T::wrapping_from` compute modulo 2ⁿ; `T::checked_add`, `T::checked_sub` and `T::checked_mul` return `option::none` instead of overflowing.
- Compilation of the v0.2 types to WasmGC (§ v2.6): structs and arrays in one recursion group, variants as tagged struct subtypes, options as nullable references, string literals and cases without payload in shared globals, long literals in data segments, and helpers for string comparison and concatenation, UTF-8 validation, `string::chars`, `string::from_chars`, `char::from_u32` and list copies.
- Values of every v0.2 type cross the component boundary (§ v2.7): each exported function that takes or returns a heap value gets an adapter that copies it through linear memory, with a bump allocator (`cm32p2_realloc`) and a post-return function; the memory only exists when a function needs it. Types-only interfaces work both imported implicitly and exported.
- `weasel call` reads its arguments and prints its result in WAVE (§ v2.9.1), for values of every v0.2 type: `weasel call invoices::total '{number: 42, …}'` prints `ok(1170)`. Invalid arguments are reported with their WaSL type and the reason.

### Changed

- `waslc` and `weasel` 0.2.0.
- `match`, `case`, `record`, `enum`, `variant`, `flags`, `char`, `string`, `list`, `tuple`, `option` and `result` are keywords; using a keyword as an identifier reports E0005.
- Integer arithmetic and conversions give the exact result or trap with `integer overflow` (§ v2.5.6), instead of wrapping silently: `+`, `-`, `*` and unary `-` on integers, conversions between integer types whose value does not fit, and float to integer conversions out of range. A NaN converted to an integer traps with `invalid conversion to integer` instead of giving `0`. The `fib` of the README now traps from `n = 93`, since it also computes fib(n + 1), where it used to return a wrong number from `n = 94`.
- E0413: a call used as a statement must call a function without a return value (§ v2.4.16). `incr();` on a function that returns a value is now an error, so the error of a `result` can no longer be lost silently; write `_ = incr();` to discard it.
- A local of an enum or `option` type can be declared without an initial value (§ v2.4.13): it starts at the default value of its Wasm type, the first case of the enum (index 0) or `option::none` (null). E0410 is reported for the other heap types, whose Wasm references cannot be null.
- The initial value of a global that is not a literal reports E0409 instead of E0100.
- `weasel` creates its wasmtime engine with WasmGC enabled explicitly (§ v2.9.2).
- `weasel call` prints NaN as `nan`, as WAVE requires.

### Removed

- E0205: exported functions have no parameter limit, parameters beyond 16 flat values go through linear memory (§ v2.7.3).

## [0.1.0] - 2026-10-11

First version of the language, implementing the [v0.1 specification](docs/spec_v0.1/README.md).

### Added

- Specification v0.1: language, mapping to WebAssembly and to the Component Model, diagnostics and tooling.
- `waslc` 0.1.0, the compiler library: numbers, booleans, functions, globals, control flow, `interface`s and `world`s, compiled into WebAssembly components with their generated WIT.
- `weasel` 0.1.0, the build tool: `init`, `build`, `check`, `wit`, `call` and `clean`.
- The `calc` example project.
- Continuous integration building and testing `waslc` and `weasel`.

[Unreleased]: https://github.com/kmuck/wasl/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/kmuck/wasl/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/kmuck/wasl/releases/tag/v0.1.0
