# Roadmap

This is the direction WaSL follows from one version to the next. It is a guideline, not a commitment: versions may be reordered or split when the implementation teaches something new. It supersedes the list of planned features in [chapter 16 of the v0.1 specification](docs/spec_v0.1/16-roadmap.md), which is frozen with v0.1. The current specification is [v0.4](docs/spec_v0.4/README.md).

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

### 0.2: data on the WasmGC heap (done)

- Implement the [v0.2 specification](docs/spec_v0.2/README.md), now frozen: `char`, `string`, `list<T>` and `list<mut T>`, `tuple`, `option`, `result`, `record`, `enum`, `variant`, `flags`, `match`, copies at the component boundary.
- Feed back into the specification the problems the implementation reveals: v0.2 is provisional until v0.3.

Target: [`examples/invoice`](examples/invoice), called with `weasel call`.

### 0.3: composing with other components (done)

- The v0.2 specification is frozen, and v0.1 and v0.2 are merged into one complete specification, [v0.3](docs/spec_v0.3/README.md), which receives the features below.
- `import` in worlds, and direct calls to imported functions: interfaces of other packages, and interfaces of the same package exported by another world, so that the services of one project call each other.
- WIT dependencies declared in `wasl.toml`: paths to other WaSL projects or WIT folders, and packages of a registry fetched with the `wkg` libraries.
- Worlds implementing foreign interfaces (the model of § 2.4.2), the entry points of WASI.
- Resources: `resource` with private state, `own<T>`, `borrow<T>`, method calls `c.add(x)`. Imported handles are released automatically when their local is overwritten and when the function returns, unless they were moved.
- The error propagation operator `?` on `option` and `result` (§ 16, question 1), since imports multiply `result`s.

Targets:

- two WaSL projects, `catalog` and `checkout`, composed into one component: `checkout` imports the `carts` interface of `catalog` and uses its `cart` resource;
- `hello`, a command-line program for WASI 0.2 that implements `wasi:cli/run` and prints with `wasi:cli/stdout`, run with `wasmtime run`.

### 0.4: async and WASI 0.3 (done)

- `async func`, with the async model of the Component Model: a call to an async function waits for its result, without `await`, and only an async function can call one (§ 2.8).
- `stream<T>`, `future<T>`, their writable ends and their built-in functions (§ 7.13, § 8.21); `task::return` to deliver a result before the end of the function, such as an HTTP response before its body (§ 8.21).
- Tuple destructuring, `local (a: A, b: B) = e;`, for the tuples of handles that WASI 0.3 returns (§ 9.4).
- Worlds implementing `wasi:cli/run` and `wasi:http/handler` of WASI 0.3.
- `weasel run <world>`, `weasel serve <world>` and `weasel init --template cli|http` (§ 14.2).
- Components run one async task at a time per instance; `weasel serve` gives each concurrent request its own instance (§ 13.13).

Targets: `hello` for WASI 0.3, run with `weasel run` (§ 15.7), and `hello-http`, a small HTTP service served by `weasel serve` and called with `curl` in CI (§ 15.8).

### 0.5: standard library and tooling

- A standard library written in WaSL: text and formatting, console, HTTP routing, JSON.
- `weasel test`, `weasel fmt`, `weasel build --all`, `weasel publish`.

Target: a small version of the reference use case, a project with two or three worlds (`account`, `order`…) sharing their types and business logic.

### Towards 1.0

WaSL reaches 1.0 when:

- the specification no longer changes incompatibly;
- the multi-service backend described in the [README](README.md#what-for) runs end to end;
- the standard library covers HTTP, JSON and console output.

## Small features, without a version

These land whenever a version needs them, rather than being tied to one:

- hexadecimal and binary literals, digit separators, exponents;
- `///` documentation comments copied into the WIT;
- unqualified constructors where the expected type is known (§ 16, question 2);
- structural equality of heap values (§ 16, question 4);
- type aliases, `type trailers = fields;` as in WIT, to shorten long types such as those of futures.

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

- WASI 0.3 and Component Model async in wasmtime, for 0.4 (enabled by default since wasmtime 46);
- the stackful tasks or the threads of the Component Model, enabled by default, to run many tasks in one instance and to start calls without waiting, `async f(x)` (§ 16, questions 15 and 16);
- the `gc` option of the Canonical ABI, to remove the copies at component boundaries (§ 16).
