# WaSL

**WaSL** (*WebAssembly Super Langage*) is a high-level programming language, with its tooling, that compiles to WebAssembly and nothing else.

A compiled WaSL project is a standard **`.wasm` component**: the code as Wasm instructions, with its WIT interfaces embedded. Any Component Model tool can read it, run it, or compose it with components written in other languages.

> **Status**: early stage. v0.1 (numbers, functions, control flow, interfaces and worlds), v0.2 (data on the WasmGC heap) and v0.3 (imports, resources, WASI 0.2 commands) are specified and implemented, in the [v0.3 specification](docs/spec_v0.3/README.md). The [v0.4 specification](docs/spec_v0.4/README.md) continues it and is the reference.

```wasl
world billing {
  export invoices;
}

interface invoices {

  record line {
    label: string,
    quantity: u32,
    unit_price: u64,
  }

  enum invoice_error {
    no_lines,
    zero_quantity,
  }

  func total(lines: list<line>) -> result<u64, invoice_error> {
    if (list::len(lines) == 0) {
      return result::err(invoice_error::no_lines);
    }
    local sum: u64;
    local i: u32;
    while (i < list::len(lines)) {
      if (lines[i].quantity == 0) {
        return result::err(invoice_error::zero_quantity);
      }
      sum = sum + u64(lines[i].quantity) * lines[i].unit_price;
      i = i + 1;
    }
    return result::ok(sum);
  }

}
```

```sh
weasel build billing    # → build/billing.wasm, a standard WebAssembly component
weasel call billing invoices::total '[{label: "pen", quantity: 3, unit-price: 150}]'    # → ok(450)
```

## What WaSL is

The language reinvents nothing the standards already define:

- **its types are the types of WIT**: `string`, `list`, `record`, `variant`, `option`, `result`…;
- **its structure is the structure of WIT**: a project is a package, code is organized in interfaces, and each world becomes a component;
- **its memory is WasmGC's**: data lives on the heap managed by the runtime, and a program never frees anything itself.

The WIT of a component is derived from the code, never written by hand, and no bindings are generated between the two.

## Why WaSL

A WaSL component embeds **no runtime, no garbage collector and no allocator**: it only contains the code of the program. That is what sets it apart from the other languages that compile to Wasm.

Most of those languages were designed for something else. To run in Wasm, they embed in every binary their own memory management (an allocator, often a GC, sometimes a whole engine) and a layer of glue that translates their concepts into Wasm's. The Wasm runtime already does all of that.

WaSL leaves that work to the runtime, which gives:

- **very small binaries**: a few kilobytes for a complete service;
- **very fast instantiation**: wasmtime can create an instance per request and throw it away right after;
- **good scalability**: the number of instances follows the request rate, down to zero when the service is idle;
- **few resources per request**: each instance only carries its own data, not one more runtime.

One exception: until the Component Model can pass WasmGC references from one component to another, values that cross the boundary are copied through a small area of linear memory, reset after each call. That copy will go away with the GC option of the Canonical ABI, without changing the language.

## What for

WaSL targets servers and workstations, and more precisely two kinds of programs built on WASI:

- **command-line tools**, which implement `wasi:cli/run`;
- **HTTP services**, which implement `wasi:http/handler`.

The goal is a language that is both fast and frugal with server resources, especially for I/O-bound workloads, where startup time and memory matter more than raw compute.

The typical project: a backend of several services (`account`, `order`, `product`…) in **one WaSL project**, with one world per service. The services share their types and business logic, but each one becomes its own component, served by wasmtime and instantiated on demand.

The road there, version by version, is in [ROADMAP.md](ROADMAP.md): imports and resources in 0.3, async and WASI 0.3 in 0.4, a standard library in 0.5.

## Built on

WaSL is a thin layer over five standard building blocks, developed in the open by major industry players.

| Building block | Its role for WaSL |
|---|---|
| [WebAssembly](https://webassembly.org/) | the instruction set and the binary format |
| [WasmGC](https://github.com/WebAssembly/gc) | the heap managed by the runtime, where data lives |
| [Component Model](https://github.com/WebAssembly/component-model) and WIT | types, interfaces, and composition with other components |
| [WASI](https://wasi.dev/) 0.3 | access to the outside world: console, files, HTTP, natively async |
| [wasmtime](https://wasmtime.dev/) | the reference runtime that runs the components |

WebAssembly and WasmGC are W3C standards, shipped in every major browser. The Component Model, WASI and wasmtime are developed by the Bytecode Alliance, whose members include Microsoft, Fastly, Intel and Mozilla.

## Repository

| Part | Role |
|---|---|
| [`docs/`](docs) | The language specification, one folder per version: [v0.4](docs/spec_v0.4/README.md), complete and in progress; [v0.1](docs/spec_v0.1/README.md), [v0.2](docs/spec_v0.2/README.md) and [v0.3](docs/spec_v0.3/README.md), frozen |
| [`waslc/`](waslc) | The compiler, a Rust library built on the `wasm-tools` crates. It never touches the filesystem. |
| [`weasel/`](weasel) | The build tool, in the spirit of `cargo`: `init`, `build`, `check`, `wit`, `call`, `clean`. It runs components with wasmtime. |
| [`examples/`](examples) | The target program of each version, built and run in CI |
| [`ROADMAP.md`](ROADMAP.md), [`CHANGELOG.md`](CHANGELOG.md) | Where WaSL is going, and what each release changed |

## Getting started

Requirements: a recent Rust toolchain.

```sh
cd weasel
cargo build --release
```

Then, with `weasel` on your `PATH`:

```sh
weasel init my-org:hello-world
cd hello-world
weasel build hello_world
weasel call hello_world math::add 1 2      # → 3
```

## License

WaSL is licensed under the [Apache License 2.0 with LLVM exception](LICENSE) (`Apache-2.0 WITH LLVM-exception`), like wasmtime. The exception means that programs compiled with WaSL carry no license obligation, even when they embed code from WaSL itself.
