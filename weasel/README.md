# weasel

`weasel` is the build tool of the **WaSL** language, in the spirit of `cargo`: it creates projects, compiles them into WebAssembly components with the [`waslc`](../waslc) compiler library, and runs them with [wasmtime](https://wasmtime.dev/).

## Commands

Every command except `init` must be run from the **root of a WaSL project**, the directory that contains `wasl.toml` and `src/`.

| Command | Description |
|---|---|
| `weasel init <namespace>:<name>[@<version>]` | Creates a new project in the directory `<name>` |
| `weasel build <world>` | Compiles a world into its component: `build/<world>.wasm` |
| `weasel check` | Checks the project for errors without producing components |
| `weasel wit` | Prints the WIT of the package |
| `weasel call <world> <function> [args…]` | Calls a function exported by a world |
| `weasel clean` | Removes the `build/` directory |

Run `weasel <command> --help` for the options of a command, and `weasel --version` for the version. Options have a single, long spelling: there is no `-h`, `-V` or `help` command.

### `weasel init`

```sh
weasel init my-org:hello-world
weasel init my-org:hello-world@0.2.0
```

The package is written as in WIT: a namespace and a name, in kebab-case, and an optional version (`0.1.0` by default). The project goes in a directory named after the package name, and its world is named after it too (`hello-world` → `hello_world`). The generated project:

```
hello-world/
├── wasl.toml
└── src/
    └── hello_world.wasl
```

### `weasel build`

A world is an entry point: each world is compiled separately into its own component. The world to build is always given explicitly.

```sh
weasel build calculator    # build/calculator.wasm
```

A project with three worlds produces three components, with three builds.

### `weasel call`

Compiles a world in memory, then calls one of its exported functions. The world is given first, as for `build`; the function is written as in WaSL source code, `interface::function`. Each argument is parsed according to the type of the corresponding parameter.

```sh
weasel call calculator math::fib 10          # 55
weasel call calculator math::clamp -50 0 10  # negative numbers need no escaping
```

- Only functions of the interfaces that the world exports can be called; internal interfaces are not part of any component.
- The result is printed alone on standard output; nothing is printed for a function without a result.
- A trap stops the call and is reported as an error:

```
error: `math::div` trapped: integer divide by zero
```

## Project layout

```
my-project/
├── wasl.toml        # package identity and dependencies
├── wkg.lock         # exact versions of the registry dependencies, written by weasel
├── src/             # all the `.wasl` files, in any sub-directory
└── build/           # produced by `weasel build`
```

```toml
# wasl.toml
[package]
namespace = "example"
name = "checkout"
version = "0.1.0"

[dependencies]
"example:catalog" = { path = "../catalog" }   # a WaSL project, or a directory of .wit files
"wasi:cli" = "0.2.6"                          # a WIT package from a registry
```

Every command except `init` resolves the dependencies first (specification § 3.5). Registry packages are fetched with the crates of [`wkg`](https://github.com/bytecodealliance/wasm-pkg-tools), configured by its usual file (`~/.config/wasm-pkg/config.toml`), cached by it, and pinned in `wkg.lock`.

The project rules are defined by the [specification](../docs/spec_v0.3/03-projects.md) (chapter 3), and the commands by its [chapter 14](../docs/spec_v0.3/14-tooling.md).

## Output

- Results (`call`, `wit`) are written to standard output.
- Progress messages and errors are written to standard error.
- Compilation errors are reported with their location and code:

```
src/math.wasl:3:12: error[E0400]: mismatched types: expected `u64`, found `u32`
```

- The exit code is `0` on success and `1` on any error, including a trap.

## Source layout

```
weasel/
├── Cargo.toml
├── README.md
├── src/
│   ├── main.rs        # command-line interface and commands
│   ├── project.rs     # loads a project (manifest + sources), analyzes and builds it
│   ├── manifest.rs    # reads and writes wasl.toml
│   ├── deps.rs        # resolves the dependencies: path projects, WIT directories, registries
│   ├── init.rs        # weasel init
│   ├── call.rs        # weasel call: function lookup, arguments, wasmtime and WASI
│   ├── compose.rs     # plugs the components that provide the imports of a world
│   └── error.rs       # errors reported to the user
└── tests/
    └── cli.rs         # runs the weasel binary on temporary projects
```

Example projects live in [`examples/`](../examples) at the root of the repository.

## Dependencies

| Crate | Role |
|---|---|
| [`waslc`](../waslc) | the WaSL compiler (path dependency) |
| [`wasmtime`](https://crates.io/crates/wasmtime) | runs the components |
| [`clap`](https://crates.io/crates/clap) | command-line interface |
| [`toml`](https://crates.io/crates/toml) + [`serde`](https://crates.io/crates/serde) | reads `wasl.toml` |
| [`wasm-pkg-client`](https://crates.io/crates/wasm-pkg-client) + [`wasm-pkg-core`](https://crates.io/crates/wasm-pkg-core) | fetch registry dependencies and write `wkg.lock`, as `wkg` does |
| [`tokio`](https://crates.io/crates/tokio) | runs the asynchronous registry client |

Development dependencies: [`tempfile`](https://crates.io/crates/tempfile), for the temporary projects of the tests, and [`wit-parser`](https://crates.io/crates/wit-parser) + [`wit-component`](https://crates.io/crates/wit-component), to publish test packages in a local registry.

## Planned commands

Like `build`, `run` and `serve` work on a whole component, so they always take the name of the world, even when the project defines only one.

| Command | Description |
|---|---|
| `weasel build --all` | Builds every world of the project |
| `weasel run <world> [args…]` | Runs the `wasi:cli/command` component of `<world>` |
| `weasel serve <world>` | Serves the `wasi:http/service` component of `<world>` |
| `weasel publish` | Publishes a component to a registry, with [wasm-pkg-tools](https://github.com/bytecodealliance/wasm-pkg-tools) |
| `weasel init <namespace>:<name> --template cli\|http` | Creates a project with a WASI entry point |

## Development

```sh
cargo build
cargo test
cd ../examples/calc && cargo run --manifest-path ../../weasel/Cargo.toml -- call calculator math::fib 10
```
