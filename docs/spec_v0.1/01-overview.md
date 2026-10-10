# 1. Overview

## 1.1 What WaSL is

**WaSL** (*WebAssembly Super Langage*) is a high-level programming language that targets WebAssembly exclusively. It is a thin, readable layer **directly on top of three standards**:

- **WebAssembly** (core): the instruction set and the module format. WaSL code compiles to Wasm instructions with no hidden runtime.
- **The Component Model**: the standard way to describe and assemble WebAssembly programs as *components* with typed interfaces, written in *WIT*. WaSL programs are organized exactly like WIT: packages, interfaces and worlds.
- **WasmGC**: garbage-collected structs and arrays provided by the runtime. Later versions of WaSL store their data there (v0.1 only uses numbers).

Other languages that compile to WebAssembly bring their own model and pay for it with glue code, an embedded runtime and an allocator. WaSL takes the opposite approach: the concepts of the standards *are* the concepts of the language, so a WaSL program compiles into a minimal component that composes directly with components written in any other language.

## 1.2 Design principles

These principles guide every decision of the specification. When a question is not answered explicitly, the answer must follow from them.

1. **One-to-one with the standards.** Every construct maps directly onto WebAssembly, the Component Model or WIT. When the standard defines a behavior (integer overflow, division by zero, initial value of locals, exports of a component), the language adopts it rather than inventing its own.
2. **Mutability where Wasm declares it.** A Wasm `global` is mutable or not; a Wasm `local` is always mutable. WaSL declares mutability at the same places, with the same rules (later: struct fields and array elements, as in WasmGC).
3. **No undefined behavior.** Every valid program has a defined behavior, possibly an immediate stop of the call (a *trap*).
4. **One way to do each thing.** No alternative syntax for the same construct. This keeps code uniform, readable by humans and easy to generate for AI agents.
5. **No implicit conversion, no hidden work.** Every type conversion is written; nothing runs that the code does not show.
6. **Explicit structure.** A program says what its component exports; the WIT of a component is derived from the source code, not written separately.
7. **A minimal core.** The language stays small. Convenience comes from libraries written in WaSL and from the tooling, not from special cases in the compiler.

## 1.3 Scope of v0.1

Version 0.1 is the minimal core needed to build the whole pipeline, from source code to a running component.

**In v0.1:**

- types: `bool`, `s8`, `s16`, `s32`, `s64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64` (chapter 7);
- declarations: `world` with `export`, `interface`, `global` (immutable or `mut`), `func` (chapter 6);
- statements: `local`, assignment, `if` / `else if` / `else`, `while`, `break`, `continue`, `return`, function calls (chapter 5);
- expressions: arithmetic, bitwise, comparison and logical operators, conversions such as `u64(x)` (chapter 8);
- one component per world, built with `weasel build <world>`, callable with `weasel call` (chapter 14).

**Not in v0.1** (chapter 16): imports, WASI, strings, records, lists and every other WasmGC-based type, resources, async, implementations of interfaces defined in other packages. The model for the last one is specified in § 2.4 so that the structure of the language is complete.

A v0.1 component uses neither WasmGC, nor linear memory, nor imports: it exchanges only scalars with its host.

## 1.4 The pipeline

```
wasl.toml + src/**/*.wasl
        │
        ▼
   waslc::analyze      lexer → parser → semantic analysis   (the whole package)
        │
        ▼
   Analysis::build      code generation → component          (one world)
        │
        ▼
 build/<world>.wasm     a WebAssembly component, with its WIT embedded
        │
        ▼
     wasmtime           weasel call math::fib 10
```

- [`waslc`](../../waslc) is a Rust library: it receives the package identity and the source texts, and returns diagnostics, the WIT of the package and components. It never touches the filesystem.
- [`weasel`](../../weasel) is the command-line tool: it reads the project from disk, calls `waslc`, writes the components and runs them with wasmtime.

## 1.5 Glossary

| Term | Meaning |
|---|---|
| **Wasm module** (core module) | A WebAssembly binary in the original format: functions, globals, memories, imports and exports of numeric types. |
| **Component** | A WebAssembly binary in the Component Model format. It wraps core modules and describes its imports and exports with rich types. WaSL produces components. |
| **WIT** | *Wasm Interface Type*, the text language that describes the interfaces and worlds of components. |
| **Package** | A named, versioned unit of WIT definitions, written `namespace:name@version` (e.g. `example:calc@0.1.0`). A WaSL project is a package. |
| **Interface** | A named group of functions (and, later, types). In WaSL an interface also contains the implementation of its functions. |
| **World** | The description of a component: what it exports (and, later, imports). In WaSL each world is compiled into one component; a world is an **entry point**. |
| **Export** | An interface that a component provides to its host or to other components. |
| **Import** | An interface that a component needs and that its host or another component provides (not in v0.1). |
| **Canonical ABI** | The rules that translate component-level values into core Wasm values when crossing the boundary of a component. |
| **Host** | The program that runs a component, here wasmtime (directly or through `weasel`). |
| **Instance** | A running copy of a component, with its own globals. |
| **Trap** | An immediate stop of the current call, reported to the host as an error (division by zero, for instance). |
| **WASI** | The *WebAssembly System Interface*: standard WIT packages (`wasi:cli`, `wasi:http`, `wasi:filesystem`…) that components use to interact with the outside world. WaSL targets WASI 0.3. |
| **wkg** | The command-line tool of `wasm-pkg-tools`, used to fetch and publish WIT packages and components. |
