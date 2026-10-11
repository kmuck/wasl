# WaSL

**WaSL** (*WebAssembly Super Langage*) is a programming language for the WebAssembly era of the cloud. It sits **one-to-one on top of WebAssembly, the Component Model and WasmGC**, and compiles programs into small, standard WebAssembly components that start in microseconds and plug into any runtime or any other component.

> **Status**: early stage. Version 0.1 (numbers, functions, globals, control flow, interfaces and worlds) is specified and implemented. See the [specification](docs/spec_v0.1/README.md). Version 0.2 (data on the WasmGC heap) is [specified](docs/spec_v0.2/README.md), provisionally until v0.3, and not implemented yet.

```wasl
world calculator {
  export math;
}

interface math {
  func fib(n: u32) -> u64 {
    local a: u64 = 0;
    local b: u64 = 1;
    local i: u32;
    while (i < n) {
      local next: u64 = a + b;
      a = b;
      b = next;
      i = i + 1;
    }
    return a;
  }
}
```

```sh
weasel build calculator     # → build/calculator.wasm, a standard WebAssembly component
weasel call math::fib 10    # → 55
```

---

## Why WaSL

WebAssembly is becoming a universal format for server-side code: sandboxed, portable, fast to start, and now able to describe typed interfaces between programs thanks to the **Component Model** and **WASI**. But the languages that compile to it were designed for something else. They bring their own runtime, garbage collector and allocator inside every binary, and translate their own concepts into WebAssembly's through layers of glue code.

WaSL takes the opposite approach: **it starts from the standards**. Its types are the types of the Component Model, its modules are WIT interfaces and worlds, its mutability is the mutability of Wasm, and its memory is managed by the garbage collector of the runtime (WasmGC). Nothing runs that the code does not show, and nothing is embedded that the runtime already provides.

The result is a language that aims to be:

- **minimal at runtime**: no embedded runtime, allocator or collector; a component only contains the code it uses;
- **fast to start and dense**: small components that instantiate in microseconds, so that thousands of instances can share a server and idle services cost nothing;
- **natively interoperable**: a WaSL component exports and imports WIT interfaces, and composes with components written in Rust, Go, JavaScript, Python…;
- **readable**: a familiar syntax, one way to do each thing, explicit types, explicit conversions, precise errors, easy to read for humans and easy to write for AI agents.

## Guiding principles

These principles drive every design decision. They are detailed in [chapter 1 of the specification](docs/spec_v0.1/01-overview.md).

1. **One-to-one with the standards.** Every construct of WaSL maps directly onto WebAssembly, the Component Model or WIT. When the standard defines a behavior (overflow, division by zero, initial values, exports), WaSL adopts it instead of inventing its own.
2. **The structure of a program is the structure of WIT.** A project is a package; code is organized in `interface`s; a `world` declares what a component exports. The WIT of a component is derived from the code, never written by hand.
3. **One world, one component, one entry point.** Each world of a project is compiled into its own component. A project can have several entry points (a CLI, a test runner, one HTTP service per domain) that share the same types and business logic.
4. **Mutability where WebAssembly declares it.** Globals are mutable or not, locals always are, and later, struct fields and array elements will be declared mutable individually, exactly like in WasmGC.
5. **Explicit over magic.** No implicit conversion, no hidden work, no undefined behavior.
6. **The latest standards only.** WaSL targets WASI 0.3 and the newest Component Model, with their native async model.
7. **A small core, rich libraries.** The language stays small; productivity comes from libraries written in WaSL and from the tooling.

## Where WaSL is going

WaSL is meant for **backend and cloud software**: serverless functions, HTTP services, edge functions and command-line tools, especially I/O-bound workloads where startup time, memory and isolation matter more than raw compute speed.

A typical target project: an enterprise backend made of several services (`account`, `order`, `product`…) written as **one WaSL project**, with one world per service, all sharing their types and business logic, each compiled into its own component and served by wasmtime, scaled to zero when idle and instantiated per request under load.

The road there:

| Version | Content |
|---|---|
| **0.1** (done) | Numbers, functions, globals, control flow, interfaces and worlds; components callable with `weasel call` |
| **0.2** (specified) | Data on the WasmGC heap: strings, records, enums, variants, lists, options, results ([spec](docs/spec_v0.2/README.md)) |
| next | Async and WASI 0.3: `async func`, streams, futures; CLI programs (`wasi:cli/run`) and HTTP services (`wasi:http/handler`) |
| next | Dependencies and imports with `wkg`; resources |
| next | Standard library (HTTP, JSON, console), `weasel run`, `serve`, `test`, `publish` |

The detailed roadmap and the open questions are in [chapter 16 of the specification](docs/spec_v0.1/16-roadmap.md).

## Repository

```
WaSL/
├── README.md     # this document
├── docs/         # the language specification
├── waslc/        # the compiler, a Rust library
├── weasel/       # the build tool, a Rust executable
└── examples/     # example WaSL projects
```

| Part | Role |
|---|---|
| [`docs/spec_v0.1`](docs/spec_v0.1/README.md) | The complete specification of v0.1, in chapters: language, mapping to Wasm and to the Component Model, diagnostics, tooling. It is the reference for humans and AI agents working on the project. |
| [`waslc`](waslc) | The compiler. It turns WaSL sources into components with the `wasm-tools` crates (`wasm-encoder`, `wit-parser`, `wit-component`). It never touches the filesystem. |
| [`weasel`](weasel) | The build tool, in the spirit of `cargo`: `init`, `build`, `check`, `wit`, `call`, `clean`. It runs components with [wasmtime](https://wasmtime.dev/). |
| [`examples`](examples) | Runnable projects, also used by the tests. |

Each Rust project is independent, with its own `Cargo.toml`, `.gitignore` and tests; `weasel` depends on `waslc` through a path dependency.

## Getting started

Requirements: a recent Rust toolchain.

```sh
cd weasel
cargo build --release
```

Then, with `weasel` on your `PATH`:

```sh
weasel init hello-world
cd hello-world
weasel build hello_world
weasel call math::add 1 2      # → 3
```

## Built on

- [WebAssembly](https://webassembly.org/), [WasmGC](https://github.com/WebAssembly/gc) and the [Component Model](https://github.com/WebAssembly/component-model)
- [WASI](https://wasi.dev/) 0.3
- [wasmtime](https://wasmtime.dev/), the reference runtime
- [wasm-tools](https://github.com/bytecodealliance/wasm-tools) and [wasm-pkg-tools](https://github.com/bytecodealliance/wasm-pkg-tools)

## License

WaSL is licensed under the [Apache License 2.0 with LLVM exception](LICENSE) (`Apache-2.0 WITH LLVM-exception`), like wasmtime. The exception means that programs compiled with WaSL carry no license obligation, even when they embed code from WaSL itself.
