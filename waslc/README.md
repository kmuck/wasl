# waslc

`waslc` is the compiler of the **WaSL** language, as a Rust **library**. It turns WaSL source texts into **WebAssembly components** (`.wasm`) that run on wasmtime and compose with other components.

`waslc` never touches the filesystem: it receives the package identity and the source texts, and returns the components and the WIT. Finding the project, reading `wasl.toml` and the files under `src/`, and writing `build/` is the job of [`weasel`](../weasel), the build tool.

The language is defined by the [specification](../docs/spec_v0.5/README.md); the compiler output is specified in its chapters 11 to 13. This document describes the architecture of the compiler.

## API

```rust
use waslc::{Package, SourceMap};

let package = Package::new("example", "calc", "0.1.0")?;

let mut sources = SourceMap::default();
sources.add("src/calc.wasl".into(), text);

// The WIT of the packages it depends on (§ 3.5), fetched by the caller.
let mut deps = waslc::Dependencies::default();
deps.add_wit_binary("wasi:cli", "0.2.6", &bytes)?;

// 1. Analysis of the whole package.
let analysis = waslc::analyze(&package, &sources, &deps)?;
for world in analysis.worlds() {
    // world.name, world.exports: exported interfaces and their functions;
    // world.imports: imported interfaces, `wasi:cli/stdout@0.2.6`;
    // world.implements: interfaces of other packages it implements, `wasi:cli/run@0.3.0`
}
let wit = analysis.wit()?;

// 2. Build of one world: a world is an entry point, compiled into its own component.
let bytes = analysis.build("calculator")?;
```

Errors are returned as a [`Diagnostic`](src/diag.rs) with a stable code from the specification. `diagnostic.display(&sources)` renders it as:

```
src/math.wasl:12:5: error[E0400]: mismatched types: expected `u64`, found `u32`
```

`analyze` returns every error of the compilation (§ 11.2): one per file with a syntax error; otherwise the first error of the declarations, or one per function body. Sources must be added in a deterministic order (by convention, sorted by path), since that order decides the order of the errors.

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
│   ├── deps.rs        # dependencies: the WIT of other packages, merged into one Resolve
│   ├── source.rs      # source files, positions (line, column)
│   ├── diag.rs        # diagnostics: error codes and display
│   ├── lexer.rs       # lexical analysis
│   ├── ast.rs         # syntax tree
│   ├── parser.rs      # syntax analysis
│   ├── types.rs       # primitive types of the language
│   ├── hir.rs         # typed intermediate representation
│   ├── sema.rs        # semantic analysis: AST → HIR, declarations and checks of the package
│   ├── sema/
│   │   ├── body.rs    # function bodies and initial values: names, types, control flow
│   │   └── foreign.rs # interfaces of other packages, read from the WIT of the dependencies
│   ├── codegen.rs     # HIR → core WebAssembly module: what a world uses, indices, sections
│   ├── codegen/
│   │   ├── boundary.rs # the component boundary: Canonical ABI, adapters, realloc
│   │   ├── boundary/
│   │   │   └── ends.rs # streams and futures: their built-in functions and the wait of a task
│   │   └── helpers.rs # helper functions: strings, UTF-8, list copies, integer overflow
│   ├── wit.rs         # HIR → WIT
│   └── component.rs   # core module + WIT → component
└── tests/
    ├── compile.rs     # valid and invalid programs (error codes)
    ├── run.rs         # runs the produced components with wasmtime: numbers, control flow
    ├── heap.rs        # heap types: strings, lists, records, variants
    ├── boundary.rs    # values that cross the component boundary
    ├── imports.rs     # imports of interfaces of other packages and of the package
    ├── resources.rs   # resources of the package, and their handles for other components
    ├── handles.rs     # handles of resources of other components, and their release
    ├── foreign_exports.rs # implementations of interfaces of other packages (WASI 0.2)
    ├── async.rs       # async functions and async exports
    ├── streams.rs     # streams and futures, and programs for WASI 0.3
    ├── task_return.rs # task.return
    ├── http.rs        # the HTTP target program of 0.4 against wasi:http 0.3
    └── fixtures/      # WaSL sources and WIT used by the tests
```

## Implementation choices

- **One error per unit**: every stage returns `Result<_, Diagnostic>`, and `?` propagates the first error of its unit. The units are a file for the lexer and the parser, the declarations for the start of `sema`, and a function body for the rest (§ 11.2); `analyze` collects the errors of the units into a `Vec<Diagnostic>`.
- **Named steps**: `sema::analyze` is a list of the steps of § 11.2, each a method of an `Analyzer` that completes the state of the analysis; `codegen::generate` first finds what a world uses (`Usage`), then assigns the function and global indices, then writes the sections of the module.
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
