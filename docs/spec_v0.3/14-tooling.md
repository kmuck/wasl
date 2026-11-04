# 14. Tooling

WaSL comes with two programs: the build tool `weasel`, used by developers, and the compiler library `waslc`, used by `weasel` and by any other tool.

## 14.1 `weasel`: general rules

- Every command except `init` operates on the project whose root is the **current directory**: it must contain `wasl.toml` and `src/` (§ 3.1). Otherwise:

  ```
  error: could not find `wasl.toml` in the current directory: weasel must be run from the root of a WaSL project
  error: could not find the `src` directory next to `wasl.toml`
  ```

- **Standard output** only receives results: the value returned by `call`, the WIT printed by `wit`.
- **Standard error** receives progress messages (`Built …`) and errors.
- Compilation errors use the format of § 11.1, manifest errors the format of § 11.4, other errors `error: <message>` (§ 11.5).
- **Exit code**: `0` on success, `1` on any error (including a trap during `call`), `2` for an invalid command line (unknown command, missing argument).
- `weasel help` and `weasel help <command>` describe the commands.

## 14.2 `weasel` commands

### `weasel init <name> [--namespace <namespace>]`

Creates a new project in a new directory `<name>` of the current directory.

- `<name>` is the package name and follows the rule of § 3.2 (`hello`, `todo-api`). The namespace defaults to `local`; the version is `0.1.0`.
- The project contains one world, named after the project with `-` replaced by `_` (`todo-api` → `todo_api`). If that name is a keyword or reserved word of WaSL (§ 4.4), the command fails.
- The command fails if `<name>` already exists.
- Generated files (no `.gitignore` and no other file):

```
hello-world/
├── wasl.toml
└── src/
    └── hello_world.wasl
```

```toml
[package]
namespace = "local"
name = "hello-world"
version = "0.1.0"
```

```wasl
// A world describes a component: the interfaces it exports.
world hello_world {
  export math;
}

// Every function of an exported interface is part of the component's interface.
interface math {

  func add(a: s32, b: s32) -> s32 {
    return a + b;
  }

}
```

### `weasel build <world>`

Compiles the world `<world>` into its component (§ 2.3, § 3.4).

1. The dependencies are resolved (§ 3.5), fetched if needed, and `wkg.lock` is written if it changed.
2. The whole package is analyzed; any error stops the command.
3. The component of `<world>` is generated and written to `build/<world>.wasm` (the `build/` directory is created if needed).
4. `Built \`build/<world>.wasm\`` is printed on standard error.

The component keeps its imports: `build` never composes it with the components that provide them, so that the same file can be deployed and composed by any tool (`wac`, a host). `weasel call` composes in memory (below).

The world is always given explicitly, even when the project has a single world: a world is an entry point, and one build produces one component. An unknown world is an error (E9001):

```
error[E9001]: unknown world `nope`: the package defines `app`, `other`
```

### `weasel check`

Resolves the dependencies like `build`, analyzes the whole package without generating any component, and prints `No errors in \`<name>\`` on standard error if there is no error.

### `weasel wit`

Prints the WIT of the package (§ 13.1) on standard output.

### `weasel call <function> [<arguments>…]`

Compiles the project in memory and calls an exported function with wasmtime. Nothing is written to `build/`.

**Function.** `<function>` is written as in WaSL source code:

- `interface::function` (`math::fib`), with WaSL names;
- or `function` alone (`fib`), when exactly one exported interface has a function with this name. Otherwise the command fails and lists the candidates:

  ```
  error: `add` is ambiguous, write one of: `math::add`, `extra::add`
  ```

No world is given: the function is looked up in every world of the project. An interface has a single implementation per package (§ 2.2), so every world that exports it runs the same code; `call` builds the component of the first world (in declaration order) that exports it. Only functions of exported interfaces can be called.

**Arguments.** There is one argument per parameter (`error: \`math::add\` expects 2 argument(s) (a: s32, b: s32), 1 given`). Each argument is a value in WAVE, parsed according to the type of its parameter (§ 14.5): `42`, `-1.5`, `"ACME"`, `{label: "pen", quantity: 3, unit-price: 150}`. Negative numbers need no escaping: `weasel call math::clamp -50 0 10`.

**Result.** The returned value is printed alone on standard output in WAVE, on one line, followed by a newline; nothing is printed for a function without a return value (§ 14.5).

**Traps.** A trap ends the call with an error and the exit code `1`:

```
error: `math::div` trapped: integer divide by zero
```

Each `call` runs on a fresh instance: globals start from their initial values.

**Imports** (specified, not available yet). When the world imports interfaces, `call` composes its component in memory before running it, with `wac-graph`:

- an interface of the package is provided by the component of the first world, in declaration order, that exports it; an interface of a path dependency that is a WaSL project, by the first world of that project that exports it. Their components are built in memory and composed in turn if they have imports of their own;
- an interface of WASI 0.2 that wasmtime provides (`wasi:cli`, `wasi:io`, `wasi:clocks`, `wasi:random`, `wasi:filesystem`) is provided by the host. The component's standard output and standard error both go to the standard error of `weasel`, so that standard output only receives the result (§ 14.1);
- any other import is an error: ``error: no component provides `example:catalog/carts@0.1.0`: add it as a path dependency on a WaSL project``.

Every instance of the composition is fresh for each `call`. Only the functions of the interfaces of the package can be called; the functions of a foreign export, such as `wasi:cli/run`, run with `wasmtime run` until `weasel run` exists (§ 14.4).

### `weasel clean`

Removes the `build/` directory if it exists (never `wkg.lock`), and prints `Removed \`build/\`` on standard error.

## 14.3 `waslc`: library API

`waslc` is a Rust library. It never reads or writes files: the caller provides the package identity and the sources, and receives results.

```rust
use waslc::{Package, SourceMap};

// Identity of the package, validated as in § 3.2 (errors: E0200).
let package = Package::new("example", "calc", "0.1.0")?;

// Sources, added in the order of § 3.3, with their display path.
let mut sources = SourceMap::default();
sources.add("src/calc.wasl".into(), text);

// Dependencies (§ 3.5), added after the packages they use:
let mut deps = waslc::Dependencies::default();
deps.add_wit_binary("wasi:cli", "0.2.6", &bytes)?;                // a binary WIT package, as `wkg` fetches it
deps.add_resolve("demo:base", None, resolve, package)?;         // a directory of .wit files, parsed with waslc::wit_parser
deps.include(&catalog_deps)?;                                   // the dependencies of a WaSL dependency…
deps.add_wit("example:catalog", "../catalog/wasl.toml", &wit)?;  // …then its WIT, `analysis.wit()`
// A dependency whose WIT does not declare its package, or a package at two versions: E0200.

// 1. Analysis of the whole package (chapters 4 to 9, 11).
let analysis = waslc::analyze(&package, &sources, &deps)?;

// Worlds of the package, with their exported interfaces and functions (WaSL names).
for world in analysis.worlds() { /* world.name, world.exports */ }

// WIT of the package (§ 13.1).
let wit: String = analysis.wit()?;

// 2. Component of one world (chapters 12 and 13); unknown world: E9001.
let bytes: Vec<u8> = analysis.build("calculator")?;
```

- Every error is a `Diagnostic { code, message, span }`. `diagnostic.display(&sources)` renders it with the format of § 11.1.
- `waslc::lexer::is_reserved(word)` tells whether a word is a keyword or a reserved word (§ 4.4).
- The result of `build` is a validated component (§ 13.9).

## 14.4 Planned commands

The following commands are planned ([ROADMAP.md](../../ROADMAP.md)):

| Command | Purpose |
|---|---|
| `weasel run <world> [args…]` | run a world that exports `wasi:cli/run`; in 0.3, `wasmtime run build/<world>.wasm` does it (§ 15.6) |
| `weasel serve <world>` | serve a world that exports `wasi:http/handler` |
| `weasel build --all` | build every world |
| `weasel test` | run the tests of the project |
| `weasel publish` | publish a component or a WIT package with wasm-pkg-tools |

Like `build`, `run` and `serve` always take the world explicitly.

## 14.5 Values in `weasel call`

`weasel call` reads its arguments and prints its result in **WAVE**, the *WebAssembly Value Encoding* of the Bytecode Alliance: the standard text format for component values, used by `wasmtime run --invoke` and `wasm-tools`. Values are written with **WIT names**, in `kebab-case`, because they are values of the component, not of the source code.

| Type | WAVE | Example |
|---|---|---|
| `bool`, integers | `true`, `false`; a decimal number in the range of the type, optionally signed | `true`, `-5`, `+7`, `42` |
| floats | a decimal number, optionally signed, with an optional exponent, or `inf`, `-inf`, `nan` | `2`, `-1.5`, `1e10`, `-inf`, `nan` |
| `char` | a char literal | `'A'`, `'\n'` |
| `string` | a quoted string with the escapes of § 4.8 | `"ACME"`, `"a\"b"` |
| record | fields in braces | `{label: "pen", quantity: 3, unit-price: 150}` |
| enum | the case | `paid`, `empty-customer` |
| variant | the case, with its payload in parentheses | `percent(10)`, `fixed(500)` |
| `list<T>` | elements in brackets | `[1, 2, 3]`, `[]` |
| `tuple<…>` | elements in parentheses | `(2, 1170)` |
| flags | the flags of the set in braces | `{email, urgent}`, `{}` |
| `option<T>` | `some(…)` or `none` | `some(percent(10))`, `none` |
| `result<…>` | `ok(…)`, `err(…)`, or `ok` / `err` without payload | `ok(1170)`, `err(no-lines)` |

- Each argument is one shell argument; a value that contains spaces or quotes is quoted for the shell: `weasel call invoices::summary '{number: 7, …}'`.
- The result is printed in WAVE on one line. Integers are printed in decimal (`55`, `-3`, `18446744073709551615`); floats in the shortest decimal form that reads back the same value, always with a `.` or an exponent (`3.0`, not `3`; `1.5`, `12.56636`, `1e20`, `inf`), and NaN as `nan`, as WAVE requires. Every field of a record is printed, including those whose value is `none`. Both choices are valid WAVE; they make the output complete and stable.
- A `result` is a value like any other: `err(…)` is printed on standard output with the exit code `0`. Only a trap fails the command (§ 14.2).
- WAVE has no value for a resource: a function whose parameters or result contain a resource, and the constructors, methods and static functions of resources, cannot be called with `weasel call` (``error: `carts::cart::of` cannot be called from the command line: its result contains the resource `cart` ``). They are called by other components (§ 15.5).
- An argument that is not a valid WAVE value of its type is an error: ``error: invalid value `{number: 1}` for parameter `inv` of type `invoice`: missing field "customer"``. The reason after the last colon comes from the WAVE parser, which quotes names with `"`.

## 14.6 Runtime configuration

`weasel` creates the wasmtime engine with the WasmGC proposal and its prerequisites enabled (chapter 12), for every component, whether it uses heap types or not.
