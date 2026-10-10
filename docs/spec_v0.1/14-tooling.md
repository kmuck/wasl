# 14. Tooling

WaSL v0.1 comes with two programs: the build tool `weasel`, used by developers, and the compiler library `waslc`, used by `weasel` and by any other tool.

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

1. The whole package is analyzed; any error stops the command.
2. The component of `<world>` is generated and written to `build/<world>.wasm` (the `build/` directory is created if needed).
3. `Built \`build/<world>.wasm\`` is printed on standard error.

The world is always given explicitly, even when the project has a single world: a world is an entry point, and one build produces one component. An unknown world is an error (E9001):

```
error[E9001]: unknown world `nope`: the package defines `app`, `other`
```

### `weasel check`

Analyzes the whole package without generating any component, and prints `No errors in \`<name>\`` on standard error if there is no error.

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

**Arguments.** There is one argument per parameter (`error: \`math::add\` expects 2 argument(s) (a: s32, b: s32), 1 given`). Each argument is parsed according to the type of its parameter:

| Type | Accepted text |
|---|---|
| `bool` | `true`, `false` |
| integers | a decimal number in the range of the type, optionally signed (`42`, `-5`, `+7`) |
| floats | a decimal number, optionally signed, with an optional exponent (`2`, `-1.5`, `1e10`), or `inf`, `-inf`, `NaN` |

An argument that does not fit is an error: `error: invalid value \`200\` for parameter \`a\` of type \`s8\``. Negative numbers need no escaping: `weasel call math::clamp -50 0 10`.

**Result.** The returned value is printed alone on standard output, followed by a newline; nothing is printed for a function without a return value.

| Type | Output |
|---|---|
| `bool` | `true` or `false` |
| integers | decimal (`55`, `-3`, `18446744073709551615`) |
| floats | shortest decimal form that reads back the same value, always with a `.` or an exponent (`2.0`, `1.5`, `12.56636`, `1e20`, `inf`, `NaN`) |

**Traps.** A trap ends the call with an error and the exit code `1`:

```
error: `math::div` trapped: integer divide by zero
```

Each `call` runs on a fresh instance: globals start from their initial values.

### `weasel clean`

Removes the `build/` directory if it exists, and prints `Removed \`build/\`` on standard error.

## 14.3 `waslc`: library API

`waslc` is a Rust library. It never reads or writes files: the caller provides the package identity and the sources, and receives results.

```rust
use waslc::{Package, SourceMap};

// Identity of the package, validated as in § 3.2 (errors: E0200).
let package = Package::new("example", "calc", "0.1.0")?;

// Sources, added in the order of § 3.3, with their display path.
let mut sources = SourceMap::default();
sources.add("src/calc.wasl".into(), text);

// 1. Analysis of the whole package (chapters 4 to 9, 11).
let analysis = waslc::analyze(&package, &sources)?;

// Worlds of the package, with their exported interfaces and functions (WaSL names).
for world in analysis.worlds() { /* world.name, world.exports */ }

// WIT of the package (§ 13.1).
let wit: String = analysis.wit()?;

// 2. Component of one world (chapters 12 and 13); unknown world: E9001.
let bytes: Vec<u8> = analysis.build("calculator")?;
```

- Every error is a `Diagnostic { code, message, span }`. `diagnostic.display(&sources)` renders it with the format of § 11.1.
- `waslc::lexer::is_reserved(word)` tells whether a word is a keyword or a reserved word (§ 4.4).
- The result of `build` is a validated component (§ 13.5).

## 14.4 Planned commands

The following commands are planned; they are not part of v0.1 (chapter 16):

| Command | Purpose |
|---|---|
| `weasel run <world> [args…]` | run a world that exports `wasi:cli/run` |
| `weasel serve <world>` | serve a world that exports `wasi:http/handler` |
| `weasel build --all` | build every world |
| `weasel test` | run the tests of the project |
| `weasel publish` | publish a component or a WIT package with wasm-pkg-tools |

Like `build`, `run` and `serve` always take the world explicitly.
