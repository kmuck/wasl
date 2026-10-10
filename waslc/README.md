# waslc

`waslc` is the compiler of the **WaSL** language, as a Rust **library**. It turns WaSL source texts into **WebAssembly components** (`.wasm`) that run on wasmtime and compose with other components.

`waslc` never touches the filesystem: it receives the package identity and the source texts, and returns the components and the WIT. Finding the project, reading `wasl.toml` and the files under `src/`, and writing `build/` is the job of [`weasel`](../weasel), the build tool.

The language is defined by the [v0.1 specification](../docs/spec_v0.1/README.md); the compiler output is specified in its chapters 11 to 13. This document describes the architecture of the compiler.

## API

```rust
use waslc::{Package, SourceMap};

let package = Package::new("example", "calc", "0.1.0")?;

let mut sources = SourceMap::default();
sources.add("src/calc.wasl".into(), text);

// 1. Analysis of the whole package.
let analysis = waslc::analyze(&package, &sources)?;
for world in analysis.worlds() {
    // world.name, world.exports: exported interfaces and their functions
}
let wit = analysis.wit()?;

// 2. Build of one world: a world is an entry point, compiled into its own component.
let bytes = analysis.build("calculator")?;
```

Errors are returned as a [`Diagnostic`](src/diag.rs) with a stable code from the specification. `diagnostic.display(&sources)` renders it as:

```
src/math.wasl:12:5: error[E0400]: mismatched types: expected `u64`, found `u32`
```

Analysis stops at the first error. Sources must be added in a deterministic order (by convention, sorted by path), since that order decides which error is reported first.

## Pipeline

```
package ──────────────────────────────────────────────┐
                                                       │
sources ──► lexer ──► parser ──► sema ──► HIR ──┬──► codegen ──► core module ─┐
            tokens    AST        names,         │                             ├──► component ──► .wasm
                                 types,         └──► wit ─────► WIT ──────────┘
                                 control flow
```

1. **lexer**: splits each file into tokens (keywords, identifiers, literals, punctuation) and enforces the lexical rules (`snake_case` identifiers, literal formats, reserved words).
2. **parser**: builds the syntax tree (AST) by recursive descent, following the grammar of the specification.
3. **sema** (semantic analysis): gathers the declarations of all the files, resolves names, checks types and control flow, and produces the **HIR**.
4. **HIR** (typed intermediate representation): a fully resolved and typed program. The following stages have nothing left to check.
5. **codegen**: translates the HIR into a core WebAssembly module for each world, keeping only the functions and globals reachable from its exports.
6. **wit**: produces the WIT of the package (exported interfaces and worlds).
7. **component**: attaches the WIT to the core module and wraps it into a component.

Each stage only depends on the previous one, so they can be tested separately.

## Source layout

```
waslc/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs         # public API: analyze, Analysis::build
│   ├── package.rs     # package identity (namespace:name@version) and its validation
│   ├── source.rs      # source files, positions (line, column)
│   ├── diag.rs        # diagnostics: error codes and display
│   ├── lexer.rs       # lexical analysis
│   ├── ast.rs         # syntax tree
│   ├── parser.rs      # syntax analysis
│   ├── types.rs       # primitive types of the language
│   ├── hir.rs         # typed intermediate representation
│   ├── sema.rs        # semantic analysis: AST → HIR
│   ├── codegen.rs     # HIR → core WebAssembly module
│   ├── wit.rs         # HIR → WIT
│   └── component.rs   # core module + WIT → component
└── tests/
    ├── compile.rs     # valid and invalid programs (error codes)
    ├── run.rs         # runs the produced components with wasmtime
    └── fixtures/      # WaSL sources used by the tests
```

## Implementation choices

- **Stop at the first error**: every stage returns `Result<_, Diagnostic>`, and `?` propagates the first error to the caller.
- **Deterministic order**: files are processed in the order they are given, then in text order, so a given error is always reported the same way.
- **Stable error codes** (`E0001`…), defined in `diag.rs` as in the specification.
- **No WAT text step**: the module is encoded directly in binary with `wasm-encoder`.
- **Standard WIT and naming**: the WIT is generated then parsed by `wit-parser`, which guarantees it is valid; core exports use the standard Component Model names (`cm32p2|example:calc/math@0.1|fib`); `wit-component` builds the component, as for every other language of the ecosystem.

## Dependencies

| Crate | Role |
|---|---|
| [`wasm-encoder`](https://crates.io/crates/wasm-encoder) | binary encoding of the core WebAssembly module |
| [`wit-parser`](https://crates.io/crates/wit-parser) | representation and validation of the generated WIT |
| [`wit-component`](https://crates.io/crates/wit-component) | embeds the WIT into the module and produces the component; prints the WIT |
| [`anyhow`](https://crates.io/crates/anyhow) | error type of `wit-parser` and `wit-component` |

Development dependencies (tests only):

| Crate | Role |
|---|---|
| [`wasmparser`](https://crates.io/crates/wasmparser) | validates the produced components |
| [`wasmtime`](https://crates.io/crates/wasmtime) | runs the produced components to check the semantics |

`wasm-encoder`, `wit-parser`, `wit-component` and `wasmparser` belong to the Bytecode Alliance [`wasm-tools`](https://github.com/bytecodealliance/wasm-tools) project and are released together with the same version number.

## Development

```sh
cargo build    # build the library
cargo test     # run all the tests (the first build of wasmtime takes a while)
```

To compile and run WaSL projects, use [`weasel`](../weasel).
