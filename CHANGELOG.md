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

- `ROADMAP.md`: the direction of WaSL from 0.2 to 1.0, with the target program of each version.
- `waslc` parses the whole v0.2 syntax: string and char literals, type declarations in interfaces, composite types, `match`, field accesses, indexes, record, flags, list and tuple literals. Constructs that the compiler cannot compile yet are reported as `E9002`.
- `record`, `enum`, `variant` and `flags` declarations in interfaces, and the built-in types `char`, `string`, `list`, `tuple`, `option` and `result` in signatures and declarations, with their errors (E0206 to E0212, E0305).
- The WIT of a package contains the types of its interfaces, and types-only interfaces used through `use`.

### Changed

- `match`, `case`, `record`, `enum`, `variant`, `flags`, `char`, `string`, `list`, `tuple`, `option` and `result` are keywords; using a keyword as an identifier reports E0005.
- The initial value of a global that is not a literal reports E0409 instead of E0100.

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
