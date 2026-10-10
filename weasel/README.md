# weasel

`weasel` is the build tool of the **WaSL** language, in the spirit of `cargo`: it creates projects, compiles them into WebAssembly components with the [`waslc`](../waslc) compiler library, and runs them with [wasmtime](https://wasmtime.dev/).

## Commands

Every command except `init` must be run from the **root of a WaSL project**, the directory that contains `wasl.toml` and `src/`.

| Command | Description |
|---|---|
| `weasel init <name>` | Creates a new project in the directory `<name>` |
| `weasel build <world>` | Compiles a world into its component: `build/<world>.wasm` |
| `weasel check` | Checks the project for errors without producing components |
| `weasel wit` | Prints the WIT of the package |
| `weasel call <function> [args…]` | Calls a function exported by the project |
| `weasel clean` | Removes the `build/` directory |

Run `weasel help <command>` for the options of a command.

### `weasel init`

```sh
weasel init hello-world
weasel init hello-world --namespace my-org
```

The name is the package name, in kebab-case. The world is named after the project (`hello-world` → `hello_world`). The default namespace is `local`. The generated project:

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

Compiles the project, then calls an exported function. The function is written as in WaSL source code: `interface::function`, or just `function` when no other exported interface has a function with that name. Each argument is parsed according to the type of the corresponding parameter.

```sh
weasel call math::fib 10          # 55
weasel call fib 10                # 55, if `fib` is unambiguous
weasel call math::clamp -50 0 10  # negative numbers need no escaping
```

- No world needs to be given: the function is looked up in every world of the project, and only the component of one world that exports it is built. An interface has a single implementation per package, so every world that exports it runs the same code.
- Only functions of exported interfaces can be called; internal interfaces are not part of any component.
- The result is printed alone on standard output; nothing is printed for a function without a result.
- A trap stops the call and is reported as an error:

```
error: `math::div` trapped: integer divide by zero
```

## Project layout

```
my-project/
├── wasl.toml        # package identity
├── src/             # all the `.wasl` files, in any sub-directory
└── build/           # produced by `weasel build`
```

```toml
# wasl.toml
[package]
namespace = "example"
name = "calc"
version = "0.1.0"
```

The project rules are defined by the [specification](../docs/spec_v0.1/03-projects.md) (chapter 3), and the commands by its [chapter 14](../docs/spec_v0.1/14-tooling.md).

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
│   ├── init.rs        # weasel init
│   ├── call.rs        # weasel call: function lookup, arguments, wasmtime
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

Development dependency: [`tempfile`](https://crates.io/crates/tempfile), for the temporary projects of the tests.

## Planned commands

Like `build`, `run` and `serve` work on a whole component, so they always take the name of the world, even when the project defines only one.

| Command | Description |
|---|---|
| `weasel build --all` | Builds every world of the project |
| `weasel run <world> [args…]` | Runs the `wasi:cli/command` component of `<world>` |
| `weasel serve <world>` | Serves the `wasi:http/service` component of `<world>` |
| `weasel publish` | Publishes a component to a registry, with [wasm-pkg-tools](https://github.com/bytecodealliance/wasm-pkg-tools) |
| `weasel init --template cli\|http` | Creates a project with a WASI entry point |

## Development

```sh
cargo build
cargo test
cd ../examples/calc && cargo run --manifest-path ../../weasel/Cargo.toml -- call math::fib 10
```
