# Roadmap

This is the direction WaSL follows from one version to the next. It is a guideline, not a commitment: versions may be reordered or split when the implementation teaches something new. It supersedes the list of planned features in [chapter 16 of the v0.1 specification](docs/spec_v0.1/16-roadmap.md), which is frozen with v0.1.

## How versions are built

- Each version has a **target program** in [`examples/`](examples), written before its specification (the example-driven rule of § 16.1).
- A version is **done** when its target program builds and runs in CI.
- Each version is specified in its own frozen folder, `docs/spec_vX.Y`, and released as crates `0.X.0` (see [CHANGELOG.md](CHANGELOG.md#versioning)).

## Why this order

A program that does something useful outside `weasel call` uses WASI, and WASI 0.3 needs three features at once:

- **imports**, because calling WASI means importing a foreign interface (`wasi:cli/stdout`, `wasi:http/types`…);
- **resources**, because `wasi:http` is built from them (`request`, `response`, `fields`);
- **async**, because WASI 0.3 entry points are `async func`s and data flows through `stream<T>` and `future<T>`.

Async and WASI are therefore of no use without imports and resources. WaSL first learns to talk to other components (0.3), then to the outside world through WASI (0.4).

## Versions

### 0.1: numbers and components (done)

Numbers, booleans, functions, globals, control flow, interfaces and worlds; components callable with `weasel call`.

Target: [`examples/calc`](examples/calc). Specification: [v0.1](docs/spec_v0.1/README.md).

### 0.2: data on the WasmGC heap

- Implement the [v0.2 specification](docs/spec_v0.2/README.md): `char`, `string`, `list<T>` and `list<mut T>`, `tuple`, `option`, `result`, `record`, `enum`, `variant`, `flags`, `match`, copies at the component boundary.
- Feed back into the specification the problems the implementation reveals: v0.2 is provisional until v0.3.

Target: [`examples/invoice`](examples/invoice), called with `weasel call`.

### 0.3: composing with other components

- `import` in worlds, and direct calls to imported functions.
- WIT dependencies declared in `wasl.toml` and fetched with `wkg`.
- Worlds implementing foreign interfaces (the model of § 2.4.2).
- Resources: `resource`, `own<T>`, `borrow<T>`, released automatically at the end of the scope that owns them.
- Likely an error propagation operator such as `?` (§ v2.11, question 1), since imports multiply `result`s.
- The v0.2 specification is frozen, and v0.1, v0.2 and v0.3 are merged into one complete specification.

Target: two WaSL components composed with `wac`, one importing the interface of the other, with a resource crossing the boundary.

### 0.4: async and WASI 0.3

- `async func`, `.await`, `stream<T>`, `future<T>`.
- Worlds implementing `wasi:cli/run` and `wasi:http/handler`.
- `weasel run <world>` and `weasel serve <world>`.

Target: a hello-world command-line program and a small HTTP service served by wasmtime.

### 0.5: standard library and tooling

- A standard library written in WaSL: text and formatting, console, HTTP routing, JSON.
- `weasel test`, `weasel fmt`, `weasel build --all`, `weasel publish`.
- Several errors reported per compilation.

Target: a small version of the reference use case, a project with two or three worlds (`account`, `order`…) sharing their types and business logic.

### Towards 1.0

WaSL reaches 1.0 when:

- the specification no longer changes incompatibly;
- the multi-service backend described in the [README](README.md#where-wasl-is-going) runs end to end;
- the standard library covers HTTP, JSON and console output.

## Small features, without a version

These land whenever a version needs them, rather than being tied to one:

- hexadecimal and binary literals, digit separators, exponents;
- `///` documentation comments copied into the WIT;
- unqualified constructors where the expected type is known (§ v2.11, question 2);
- structural equality of heap values (§ v2.11, question 4);
- optional overflow checks.

## Further out

What the project will need beyond the language itself:

- **Editor support**: a language server (diagnostics, go to definition) and syntax highlighting.
- **Library distribution**: source packages or components composed with `wac` (§ 16.3), and publishing to a registry through `wkg`.
- **Debugging**: debug information mapping a trap back to a source line.
- **Installation**: prebuilt `weasel` binaries attached to GitHub Releases.
- **Documentation**: a website with the specification and a tutorial, possibly a browser playground.
- **Measurements**: benchmarks of component size and instantiation time, the main promise of WaSL.

## Upstream dependencies

Some versions depend on the standards and runtimes WaSL builds on:

- WASI 0.3 and Component Model async in wasmtime, for 0.4;
- the `gc` option of the Canonical ABI, to remove the copies at component boundaries (§ v2.11, question 8).
