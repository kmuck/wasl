# CLAUDE.md

Guidance for AI agents (and humans) working on this repository. Read it before changing the language, the compiler or the tooling.

## What WaSL is

WaSL is a high-level language that compiles to WebAssembly components and nothing else. It sits **one-to-one on top of WebAssembly, WasmGC, the Component Model (WIT) and WASI**:

- its types are the WIT types (`string`, `list`, `record`, `variant`, `option`, `result`…);
- its structure is WIT's: a project is a package, code lives in `interface`s, each `world` is compiled into its own component;
- its memory is WasmGC's: data lives on the runtime's heap, no allocator or GC is embedded;
- the WIT of a component is derived from the code, never written by hand.

It targets WASI command-line tools (`wasi:cli/run`) and HTTP services (`wasi:http/handler`), and in particular a backend of several services in one project, one world per service, each instantiated on demand by wasmtime. See [README.md](README.md) for the pitch and [ROADMAP.md](ROADMAP.md) for the versions.

## Design principles, in order

When two principles conflict, the first one wins.

1. **Simple and one-to-one with the low level.** Every construct maps directly onto Wasm, WasmGC, the Component Model or WASI. Before proposing a language rule, ask *how does this work in the binary?* and show the mapping. When a standard defines a behavior, WaSL adopts it instead of inventing its own. A rule stricter than what the binary requires needs a justification.
2. **A fast compiler.** No costly or heavy analyses: prefer rules that are a single local check. A fast compile gives a short iteration loop for AI agents, which is an advantage over Rust.
3. **Easy for an AI agent.** Least ambiguity, one way to do each thing, explicit types and conversions, precise errors, problems caught at compile time. This arbitrates only where the standards leave a choice open.

Also: no new syntax that is not obviously needed, and no keywords that do not exist in the Component Model.

## Settled decisions

Do not re-propose these unless Karim asks.

- **No user-defined generics.** Genericity exists only in the WIT type constructors (`list<T>`, `option<T>`, `result<T, E>`, `tuple<…>`) and in built-in functions, like Go before 1.18. WIT has no generics, so they could not be exported.
- **No `mut` in the WIT.** WIT has no mutability: a `mut` field or a `list<mut T>` in a type that appears in the WIT is an error (E0214). There is no `with` record update; build a new record.
- **`list<T>` is immutable for everyone, `list<mut T>` is mutable**, and `list::freeze(xs)` (a copy) is the only conversion. No `vec` or `map` types for now; a growable list waits until a target program needs it, and `map<K, V>` will come from the Component Model.
- **Integer arithmetic and conversions give the exact result or trap.** Wrapping and checked arithmetic are explicit: `T::wrapping_add`, `T::checked_add`… (§ 10.2).
- **Locals get the default value of their Wasm type** when they have no initializer: numbers are zero, an enum is its first case, an option is `none`; other heap types need an initializer (E0410). No definite-assignment analysis.
- **Values are never ignored silently.** A call statement must call a function without a return value (E0413); `_ = f();` is the explicit `drop`.
- **Recursive types** are accepted when they have a value (E0211), and stay internal, since WIT forbids them (E0208).
- **Several errors per compilation, without cascades** (§ 11.2): one syntax error per file, then no analysis; the first error of the declarations stops; one error per function body. `weasel` shows the first ten, like Go.
- **Imported resource handles are released automatically** (0.3): when their local is overwritten and on every exit of the function, unless they were moved; no explicit `drop`. A resource of the package is a plain WasmGC value inside the component.
- **Resource calls** (0.3): methods are called `c.add(x)`, constructors `carts::cart(…)` like a conversion.
- **Imports** (0.3) cover interfaces of other packages and interfaces of the same package exported by another world; dependencies are fetched with the `wkg` crates.

## Repository

```
README.md       pitch, example, getting started
ROADMAP.md      versions 0.1 → 1.0, each with a target program
CHANGELOG.md    Keep a Changelog, with the versioning rules
docs/spec_vX.Y  the specification of each version, one frozen folder per release
waslc/          the compiler, a Rust library (lexer → parser → sema → HIR → codegen / wit → component)
weasel/         the build tool, a Rust executable (init, build, check, wit, call, clean), runs components with wasmtime
examples/       the target programs of each version (calc for 0.1, invoice for 0.2, catalog + checkout and hello for 0.3), run in CI
```

- `waslc` and `weasel` are **separate Cargo projects**, with no root workspace; `weasel` depends on `waslc` by path. Keep it that way.
- `waslc` never touches the filesystem: it receives sources and returns components and WIT. Files, `wasl.toml` and `build/` are `weasel`'s job.
- The architecture of the compiler is described in [waslc/README.md](waslc/README.md), the tool in [weasel/README.md](weasel/README.md).
- Error codes (`E0001`…) are stable, defined in `waslc/src/diag.rs` and listed in the diagnostics chapter of the specification. A new error gets a new code in both places. `E9999` is an internal error: a program that triggers it is a compiler bug.

## Specification

- The specification is the reference. A language change starts in the spec, and the compiler follows; behavior found during implementation is written back into the spec.
- v0.1 and v0.2 are frozen. [docs/spec_v0.3](docs/spec_v0.3/README.md) merges them into one complete specification, the reference, and receives the features of 0.3; it is **provisional until v0.4**. References are written `§ 8.2`, and point to v0.3; its README maps the old `§ v2.x.y` numbers.
- Each version is **example-driven**: its target program in `examples/` is written first, and the version is done when that program builds and runs in CI.

## Workflow

- **Trunk-based**: short branches and a pull request into `main`; no `dev` branch. CI (build + tests of both crates, and the target program run with `weasel`) must pass on every PR.
- Run the checks of each crate you touch before pushing:

  ```sh
  cd waslc && cargo build --locked --all-targets && cargo test --locked
  cd weasel && cargo build --locked --all-targets && cargo test --locked
  ```

  The first build of wasmtime takes a while.
- Add a line to the `[Unreleased]` section of [CHANGELOG.md](CHANGELOG.md) for every user-visible change (language, compiler output, error, tool).
- **Versioning**: both crates share one SemVer version. In `0.x`, the minor version is the spec version (`0.2.x` implements spec v0.2); patches only fix bugs.
- **Releases**: an annotated tag `vX.Y.Z` on `main` and a GitHub Release. Karim creates tags and releases himself; agents prepare the CHANGELOG and the version bump.
- Commit messages: a short imperative title, then a body explaining what changes and why, citing the spec sections (`§ 9.6`) and error codes involved.

## Writing style

The repository is written in English: code, comments, specification, commit messages. Prefer short, plain sentences, and examples over abstractions.
