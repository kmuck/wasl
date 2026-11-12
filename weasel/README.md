# weasel

`weasel` is the build tool of the **WaSL** language, in the spirit of `cargo`: it creates projects, compiles them into WebAssembly components with the [`waslc`](../waslc) compiler library, and runs them with [wasmtime](https://wasmtime.dev/).

## Commands

Every command except `init` must be run from the **root of a WaSL project**, the directory that contains `wasl.toml` and `src/`.

| Command | Description |
|---|---|
| `weasel init <namespace>:<name>[@<version>] [--template lib\|cli\|http]` | Creates a new project in the directory `<name>` |
| `weasel build <world>` | Compiles a world into its component: `build/<world>.wasm` |
| `weasel check` | Checks the project for errors without producing components |
| `weasel wit` | Prints the WIT of the package |
| `weasel call <world> <function> [args…]` | Calls a function exported by a world |
| `weasel run <world> [args…]` | Runs a world as a command, through its export of `wasi:cli/run` |
| `weasel serve <world> [--addr <host>:<port>]` | Serves a world over HTTP, through its export of `wasi:http/handler` |
| `weasel test [<filter>]` | Runs the tests of `tests/`, each in a new instance |
| `weasel clean` | Removes the `build/` directory |

Run `weasel <command> --help` for the options of a command, and `weasel --version` for the version. Options have a single, long spelling: there is no `-h`, `-V` or `help` command.

### `weasel init`

```sh
weasel init my-org:hello-world
weasel init my-org:hello-world@0.2.0
weasel init my-org:greet --template cli    # a command of WASI 0.3, for weasel run
weasel init my-org:api --template http     # an HTTP service of WASI 0.3, for weasel serve
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

- An async function is called like the others: `call` prints its result once its task has ended.

### `weasel run`

Compiles a world in memory and runs it as a command: the `run` function of its export of `wasi:cli/run`, of WASI 0.2 or 0.3. Every argument after the world goes to the program unchanged, after the name of the world; the program has the standard streams of `weasel`, and `weasel` exits with its code.

```sh
weasel run hello            # Hello, world!
weasel run args a --help    # the program receives ["args", "a", "--help"]
```

### `weasel serve`

Compiles a world in memory and serves it over HTTP: each request goes to the `handle` function of its export of `wasi:http/handler`, of WASI 0.3, in a new instance. A trap, or an error returned by `handle`, before the response gives the status 500 and is printed on standard error. `SIGINT` (Ctrl-C) or `SIGTERM` stops the service.

```sh
weasel serve hello_http &                   # Serving `hello_http` on http://127.0.0.1:8080
curl http://127.0.0.1:8080/karim            # Hello, /karim!
weasel serve hello_http --addr 0.0.0.0:3000
```

### `weasel test`

Analyzes the package with its test files, the `.wasl` files of `tests/`, then runs every test, each in a new instance of a component built in memory. A test is a function of a test interface with no parameter that returns `result<_, string>`; the other functions are helpers. A test file declares interfaces, which can call every interface of the package; the package cannot call them, and `build` and `check` ignore them.

```sh
weasel test                 # every test
weasel test total           # the tests whose name contains `total`
```

```
ok    shop_test::total_of_two_lines
FAIL  shop_test::parses_an_empty_order: expected no line
FAIL  shop_test::rejects_zero: trapped: integer divide by zero

1 passed, 2 failed
```

The exit code is `1` when a test fails. What the tests write to their standard output goes to the standard error of `weasel`. In 0.5, a test can call the interfaces of WASI, but not those of other packages.

The components that `call`, `run`, `serve` and `test` run get WASI 0.2 and 0.3 and `wasi:http` 0.3 from the host, without environment variables, filesystem or network: sockets are refused and outgoing HTTP requests are denied.

## Project layout

```
my-project/
├── wasl.toml        # package identity and dependencies
├── wkg.lock         # exact versions of the registry dependencies, written by weasel
├── src/             # all the `.wasl` files, in any sub-directory
├── tests/           # the test files, run by `weasel test`
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

The project rules are defined by the [specification](../docs/spec_v0.5/03-projects.md) (chapter 3), and the commands by its [chapter 14](../docs/spec_v0.5/14-tooling.md).

## Output

- Results (`call`, `wit`, `test`) and the output of the program of `run` are written to standard output.
- Progress messages and errors are written to standard error.
- Compilation errors are reported with their location and code:

```
src/math.wasl:3:12: error[E0400]: mismatched types: expected `u64`, found `u32`
```

- The exit code is `0` on success and `1` on any error, including a trap; `run` exits with the code of its program.

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
│   ├── call.rs        # weasel call: function lookup, arguments, results
│   ├── run.rs         # weasel run: commands of wasi:cli/run
│   ├── serve.rs       # weasel serve: HTTP server, one instance per request
│   ├── test.rs        # weasel test: runs the tests, one instance per test
│   ├── host.rs        # wasmtime engine and host: WASI 0.2 and 0.3, wasi:http 0.3
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
| [`wasmtime`](https://crates.io/crates/wasmtime) + [`wasmtime-wasi`](https://crates.io/crates/wasmtime-wasi) + [`wasmtime-wasi-http`](https://crates.io/crates/wasmtime-wasi-http) | run the components, with WASI and `wasi:http` |
| [`wac-graph`](https://crates.io/crates/wac-graph) | composes a component with the components that provide its imports |
| [`hyper`](https://crates.io/crates/hyper) + [`http-body-util`](https://crates.io/crates/http-body-util) + [`futures`](https://crates.io/crates/futures) | the HTTP server of `serve` |
| [`clap`](https://crates.io/crates/clap) | command-line interface |
| [`toml`](https://crates.io/crates/toml) + [`serde`](https://crates.io/crates/serde) | reads `wasl.toml` |
| [`wasm-pkg-client`](https://crates.io/crates/wasm-pkg-client) + [`wasm-pkg-core`](https://crates.io/crates/wasm-pkg-core) | fetch registry dependencies and write `wkg.lock`, as `wkg` does |
| [`tokio`](https://crates.io/crates/tokio) | runs the asynchronous registry client, components and server |

Development dependencies: [`tempfile`](https://crates.io/crates/tempfile), for the temporary projects of the tests, and [`wit-parser`](https://crates.io/crates/wit-parser) + [`wit-component`](https://crates.io/crates/wit-component), to publish test packages in a local registry.

## Planned commands

| Command | Description |
|---|---|
| `weasel build --all` | Builds every world of the project |
| `weasel publish` | Publishes a component to a registry, with [wasm-pkg-tools](https://github.com/bytecodealliance/wasm-pkg-tools) |

## Development

```sh
cargo build
cargo test
cd ../examples/calc && cargo run --manifest-path ../../weasel/Cargo.toml -- call calculator math::fib 10
```
