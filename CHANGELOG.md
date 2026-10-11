# Changelog

All notable changes to WaSL are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## Versioning

- The **language** is versioned by its specification: `docs/spec_v0.1`, `docs/spec_v0.2`…
- The two crates, [`waslc`](waslc) and [`weasel`](weasel), follow [Semantic Versioning](https://semver.org/) and are always released together with the **same version number**.
- While WaSL is in `0.x`, the minor version of the crates is the version of the specification they implement: `0.2.x` implements the v0.2 specification. Patch releases (`0.2.1`, `0.2.2`…) only fix bugs and never change the language.
- Each release is a git tag `vX.Y.Z` on `main`, with a matching GitHub Release.

## [Unreleased]

## [0.2.0] - 2026-10-11

Data on the WasmGC heap, implementing the [v0.2 specification](docs/spec_v0.2/README.md) (provisional until v0.3). The target program, [`examples/invoice`](examples/invoice), builds and runs in the continuous integration.

### Added

- `ROADMAP.md`: the direction of WaSL from 0.2 to 1.0, with the target program of each version.
- `waslc` parses the whole v0.2 syntax: string and char literals, type declarations in interfaces, composite types, `match`, field accesses, indexes, record, flags, list and tuple literals.
- `record`, `enum`, `variant` and `flags` declarations in interfaces, and the built-in types `char`, `string`, `list`, `tuple`, `option` and `result` in signatures and declarations, with their errors (E0206 to E0212, E0305).
- The WIT of a package contains the types of its interfaces, and types-only interfaces used through `use`.
- Typing of the v0.2 expressions and statements: string, char, record, flags, list and tuple literals, constructors, field accesses, indexes, `match`, the operators on chars, strings, enums and flags, the built-in functions, constant globals and assignments to `mut` fields and `list<mut T>` elements, with their errors (E0306, E0307, E0406 to E0412, E0505 to E0507). `examples/invoice` passes semantic analysis and `weasel wit` prints its WIT.
- Compilation of the v0.2 types to WasmGC (§ v2.6): structs and arrays in one recursion group, variants as tagged struct subtypes, options as nullable references, string literals and cases without payload in shared globals, long literals in data segments, and helpers for string comparison and concatenation, UTF-8 validation, `string::chars`, `string::from_chars`, `char::from_u32` and list copies.
- Values of every v0.2 type cross the component boundary (§ v2.7): each exported function that takes or returns a heap value gets an adapter that copies it through linear memory, with a bump allocator (`cm32p2_realloc`) and a post-return function; the memory only exists when a function needs it. `examples/invoice` builds and runs. Types-only interfaces work both imported implicitly and exported (open question 6).
- `weasel call` reads its arguments and prints its result in WAVE (§ v2.9.1), for values of every v0.2 type: `weasel call invoices::total '{number: 42, …}'` prints `ok(1170)`. Invalid arguments are reported with their WaSL type and the reason.

### Changed

- `match`, `case`, `record`, `enum`, `variant`, `flags`, `char`, `string`, `list`, `tuple`, `option` and `result` are keywords; using a keyword as an identifier reports E0005.
- The initial value of a global that is not a literal reports E0409 instead of E0100.

- `waslc` and `weasel` 0.2.0.
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

[Unreleased]: https://github.com/kmuck/wasl/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/kmuck/wasl/releases/tag/v0.1.0
