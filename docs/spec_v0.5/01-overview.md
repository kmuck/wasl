# 1. Overview

## 1.1 What WaSL is

**WaSL** (*WebAssembly Super Langage*) is a high-level programming language that targets WebAssembly exclusively. It is a thin, readable layer **directly on top of three standards**:

- **WebAssembly** (core): the instruction set and the module format. WaSL code compiles to Wasm instructions with no hidden runtime.
- **The Component Model**: the standard way to describe and assemble WebAssembly programs as *components* with typed interfaces, written in *WIT*. WaSL programs are organized exactly like WIT: packages, interfaces and worlds, and the types of WaSL are the types of WIT.
- **WasmGC**: garbage-collected structs and arrays provided by the runtime. Strings, lists, records and the other heap types of WaSL live there (§ 7.5).

Other languages that compile to WebAssembly bring their own model and pay for it with glue code, an embedded runtime and an allocator. WaSL takes the opposite approach: the concepts of the standards *are* the concepts of the language, so a WaSL program compiles into a minimal component that composes directly with components written in any other language.

## 1.2 Design principles

These principles guide every decision of the specification. When a question is not answered explicitly, the answer must follow from them.

1. **One-to-one with the standards.** Every construct maps directly onto WebAssembly, WasmGC, the Component Model or WIT. When the standard defines a behavior (division by zero, initial value of locals, exports of a component), the language adopts it rather than inventing its own. Integer overflow is the one place where WaSL chooses among the behaviors Wasm offers (§ 1.3, decision 8).
2. **Mutability where Wasm declares it.** A Wasm `global` is mutable or not; a Wasm `local` is always mutable; a WasmGC struct field or array element is mutable or not. WaSL declares mutability at the same places, with the same rules.
3. **No undefined behavior.** Every valid program has a defined behavior, possibly an immediate stop of the call (a *trap*).
4. **One way to do each thing.** No alternative syntax for the same construct. This keeps code uniform, readable by humans and easy to generate for AI agents. The one exception is readability's: the body of an `if`, `else`, `while` or `for` can be a single simple statement without braces (§ 5.3), and `weasel fmt` gives both forms one layout.
5. **No implicit conversion, no hidden work.** Every type conversion is written; nothing runs that the code does not show.
6. **Explicit structure.** A program says what its component exports; the WIT of a component is derived from the source code, not written separately.
7. **A minimal core.** The language stays small. Convenience comes from libraries written in WaSL and from the tooling, not from special cases in the compiler.

## 1.3 Design decisions

Each decision follows from the principles of § 1.2. The rest of this specification details them.

1. **The types are the types of WIT.** Each type has the name, the cases and the meaning of the WIT type of the same name; the WIT of a package is derived by erasure (§ 2.6). Types are declared **inside interfaces**, as in WIT, and named from another interface with a path, `invoices::line`.
2. **Values live on the WasmGC heap.** A record is a WasmGC `struct`, a list a WasmGC `array`, a string an `array` of UTF-8 bytes. The component embeds no allocator and no collector: the runtime provides both (§ 1.1). Enums, flags and chars are plain integers, as in the Canonical ABI.
3. **Reference semantics inside a component.** Assigning a record, a list or a string, or passing it to a function, copies a reference, as for objects in Java or JavaScript. Nothing is copied implicitly inside a component.
4. **Copies at the boundary only.** A value that crosses the boundary of a component is deep-copied through linear memory, as the Canonical ABI requires today. Linear memory is used **only** as a scratch buffer for these copies, managed by a bump allocator that is reset after every call, with one region per task (§ 13.7). When runtimes support the `gc` option of the Canonical ABI, these copies disappear without changing the language (§ 16).
5. **Mutability where WasmGC declares it.** A record field is immutable unless declared `mut`, exactly like a WasmGC struct field (§ 1.2, principle 2). A `list<T>` is immutable, for everyone and for ever, and only the elements of a `list<mut T>` can be assigned (§ 7.10). Strings and tuples are immutable. Mutability stays inside the component: the WIT has none, so the types that appear in it have no `mut` field and no `list<mut T>` (§ 7.11).
6. **No null, and the defaults of Wasm.** Every value of a heap type is a valid value: there is no `null`, and the absence of a value is written `option<T>`, whose `none` is the null reference of Wasm. A local without initial value takes the default value of its Wasm type, which only scalars and options have: `0`, the first case of an enum, `option.none`. A local of another heap type needs an initial value (§ 9.4).
7. **One way to write each thing.** A record literal lists its fields in declaration order; every constructor is written with its type (`option.some(x)`, `status::paid`); a variant is inspected only with `match`.
8. **An integer is exact, or the call stops.** Integer arithmetic and conversions trap when their exact result does not fit in its type, instead of wrapping (§ 10.2). Wrapping remains available when it is written: `u32.wrapping_add(a, b)` is the plain `i32.add`. Wasm has both wrapping instructions (`i32.add`) and trapping ones (`i32.div_s`, `i32.trunc_f64_s`), and the `+` of WaSL checks its result.
9. **A call statement discards its value.** Wasm removes an unused value with `drop`; WaSL emits it for a call used as a statement, `f();` (§ 9.6). An error is propagated with `?` or handled with `match`. Until 0.4, `_ = f();` was required for that `drop`; it added noise to every line for little information, and was removed in 0.5.
10. **Async is the Component Model's.** An `async func` is the `async func` of WIT. A call to it waits for its result, as the synchronous lowering of the Component Model does, and nothing marks the call: there is no `await` (§ 2.8). Only an `async func` can call one, which is the rule of the Component Model, checked at compile time. Streams and futures are the `stream<T>` and `future<T>` of WIT, with their writable ends (§ 7.13), and `task.return` is the `task.return` of the Component Model, which delivers the result of a task before its end (§ 8.21). Async functions, streams, futures and `task.return` are available.
11. **Built-in names and declared names look different.** A built-in function or constructor is written with its type and a `.`, `list.len(xs)`, `result.ok(x)`, `u32.to_string(n)`, `task.return(v)`; `::` is only for names declared in code, `invoices::total(…)`, `status::paid`; `c.add(x)` is only a method of a resource (§ 5.3, § 8.17).
12. **Types are written where they form a contract.** Parameters, return types, fields and globals always have their type, since they are the WIT of the code; a local can take the type of its value, `local n = list.len(xs);` (§ 9.4).
13. **A standard library above WASI.** `wasl:std` gives programs text, JSON, files, HTTP services and commands without the streams and resources of WASI, and is written in WaSL (chapter 17). WASI stays one `import` away for what the library does not cover.

## 1.4 Scope

**In this version:**

- primitive types: `bool`, `s8`, `s16`, `s32`, `s64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64` (§ 7.1);
- the built-in types `char`, `string`, `list<T>`, `tuple<…>`, `option<T>` and `result<T, E>`, which have no declaration (§ 7.6);
- the declarations `record`, `enum`, `variant` and `flags`, written inside interfaces like in WIT (§ 7.7);
- declarations: `world` with `export`, `interface`, `global` (immutable or `mut`), `func` (chapter 6);
- statements: `local`, assignment, `if` / `else if` / `else`, `while`, `break`, `continue`, `return`, `match`, function calls (chapter 5);
- expressions: arithmetic, bitwise, comparison and logical operators, conversions such as `u64(x)`, string, char, record, flags, list and tuple literals, constructors, field access `r.f`, tuple access `t.0`, indexing `xs[i]` (chapter 8);
- a small set of built-in functions on strings, chars, lists and integers (§ 8.17);
- integer arithmetic and conversions that give the exact result or trap, with explicit wrapping and checked functions on each integer type (§ 10.2, § 8.17);
- one component per world, built with `weasel build <world>`, whose exported functions take and return values of every type through the Canonical ABI, callable with `weasel call` (chapters 13 and 14).

With these, every WIT type except resources (`own`, `borrow`) and async types (`stream`, `future`) has its WaSL equivalent.

**Added in 0.3**:

- dependencies on other packages, declared in `wasl.toml` and resolved from a path or a registry (§ 3.2, § 3.5);
- imports of interfaces of other packages and of the same package (§ 2.5), and implementations of interfaces defined by other packages, such as `wasi:cli/run` (§ 2.4.2);
- resources: their declaration, constructors, methods and static functions (§ 7.12), handles and their automatic release (§ 9.9);
- the error propagation operator `?` on `result` and `option` (§ 8.19).

With these, every WIT type except async types (`stream`, `future`) has its WaSL equivalent, and a WaSL component can run as a WASI 0.2 command (§ 15.6).

**Added in 0.4**:

- async functions: their declaration, calls that wait without a marker, and the rule that only an async function calls one (§ 2.8, § 8.21);
- `stream<T>` and `future<T>`, their writable ends `stream_writer<T>` and `future_writer<T>`, and their built-in functions (§ 7.13, § 8.21);
- `task.return`, which delivers the result of an exported async function before its end (§ 8.21);
- tuple destructuring, `local (a: A, b: B) = e;`, which gives each handle of a tuple its own local (§ 9.4, § 7.12);
- WIT names with uppercase letters, written in lowercase in WaSL (§ 13.2);
- the implementation of async foreign interfaces, such as `wasi:cli/run` and `wasi:http/handler` of WASI 0.3 (§ 13.12);
- `weasel run` and `weasel serve`, which run a command and serve an HTTP service, and `weasel call` of async functions (§ 14.2);
- `weasel init --template cli|http`, which starts a project from the command or the service of 0.4 (§ 14.2).

With these, every WIT type except `map<K, V>`, `error-context`, and `stream` and `future` without an element type has its WaSL equivalent, and a WaSL component can run as a WASI 0.3 command (§ 15.7) or HTTP service (§ 15.8).

**Added in 0.5**, available:

- locals without a type, `local x = e;`, and in destructurings (§ 9.4);
- `for (x in xs)` on lists (§ 9.10), with braces until bodies without braces are available.

**Added in 0.5** (*specified, not available yet*):

- bodies without braces: a single simple statement after `if`, `else`, `while` and `for` (§ 5.3);
- built-in functions written `type.f(…)`, with `T.to_string` and `T.parse` on integers (§ 8.17);
- call statements that discard their value, without `_ =` (§ 9.6);
- structural `==` on records, tuples, lists, variants, options and results (§ 8.3);
- type aliases, `type name = T;` (§ 7.14), and doc comments, `///` (§ 4.2);
- `use` and `use … as`, which name the interfaces of other packages in a file (§ 6.6);
- functions without body in the interfaces of a library and worlds of a library, which a world includes with `include` and whose functions it writes (§ 2.4.3);
- the standard library `wasl:std`: `text`, `json`, `http`, `cli` and `fs`, with the worlds `service` and `command` and their entry points `handle` and `run` (§ 2.9, chapter 17);
- `weasel build --all`, `weasel test` with the tests of `tests/`, `weasel fmt` and `fmt --check`, `weasel run --dir` (§ 3.6, § 14.2).

**Not yet available** ([ROADMAP.md](../../ROADMAP.md), chapter 16): libraries written by users, concurrency inside a task (`async f(x)`, § 16), cancellation, string formatting, a growable list and `map<K, V>`, outgoing HTTP requests in the standard library.

A component whose exported functions only exchange scalars uses neither WasmGC at the boundary, nor linear memory, nor imports (§ 13.6).

## 1.5 The pipeline

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
     wasmtime           weasel call calculator math::fib 10
```

- [`waslc`](../../waslc) is a Rust library: it receives the package identity and the source texts, and returns diagnostics, the WIT of the package and components. It never touches the filesystem.
- [`weasel`](../../weasel) is the command-line tool: it reads the project from disk, calls `waslc`, writes the components and runs them with wasmtime.

## 1.6 Glossary

| Term | Meaning |
|---|---|
| **Wasm module** (core module) | A WebAssembly binary in the original format: functions, globals, memories, imports and exports. |
| **Component** | A WebAssembly binary in the Component Model format. It wraps core modules and describes its imports and exports with rich types. WaSL produces components. |
| **WIT** | *Wasm Interface Type*, the text language that describes the interfaces and worlds of components. |
| **Package** | A named, versioned unit of WIT definitions, written `namespace:name@version` (e.g. `example:calc@0.1.0`). A WaSL project is a package. |
| **Interface** | A named group of functions and types. In WaSL an interface also contains the implementation of its functions. |
| **World** | The description of a component: what it imports and what it exports. In WaSL each world is compiled into one component; a world is an **entry point**. |
| **Export** | An interface that a component provides to its host or to other components. |
| **Import** | An interface that a component needs and that its host or another component provides (§ 2.5). |
| **Resource** | A Component Model type with an identity and methods, reached from outside its component only through a handle (§ 7.12). |
| **Handle** | An `i32` that designates a resource owned by another component, or an end of a stream or a future, in the handle table of the current instance (§ 9.9). |
| **Task** | One call of an exported function, as the Component Model runs it. It delivers its result once, with `task.return`, and may keep running after (§ 2.8). |
| **Async function** | A function declared `async func`, the `async func` of WIT: a call to it may wait, and other tasks may run while it waits (§ 2.8). |
| **Stream**, **future** | Values of the Component Model that carry data from one component to another over time: a sequence of values for `stream<T>`, one value for `future<T>`. Each has a readable end and a writable end, both handles (§ 7.13). |
| **Dependency** | Another package whose interfaces a project imports or implements, declared in `wasl.toml` (§ 3.5). |
| **Canonical ABI** | The rules that translate component-level values into core Wasm values when crossing the boundary of a component. |
| **WasmGC** | The garbage collection proposal of WebAssembly, part of WebAssembly 3.0: typed structs and arrays allocated and reclaimed by the runtime. |
| **Scalar type**, **heap type** | A scalar type is represented by a Wasm number; a heap type by a reference to a WasmGC object (§ 7.2). |
| **Host** | The program that runs a component, here wasmtime (directly or through `weasel`). |
| **Instance** | A running copy of a component, with its own globals and its own heap objects. |
| **Trap** | An immediate stop of the current call, reported to the host as an error (division by zero, for instance). |
| **WASI** | The *WebAssembly System Interface*: standard WIT packages (`wasi:cli`, `wasi:http`, `wasi:filesystem`…) that components use to interact with the outside world. |
| **WAVE** | The *WebAssembly Value Encoding*: the standard text format of component values, used by `weasel call` (§ 14.5). |
| **wkg** | The command-line tool of `wasm-pkg-tools`, used to fetch and publish WIT packages and components. |

## 1.7 Changes from v0.1

Version 0.2 accepts every valid v0.1 program with the same behavior, with these exceptions, which hold in this specification:

| Change | Effect on v0.1 programs |
|---|---|
| `record`, `enum`, `variant`, `flags`, `match`, `case`, `char`, `string`, `list`, `tuple`, `option` and `result` are keywords (§ 4.4) | an identifier named `match` or `case`, which v0.1 did not reserve, is now E0005 |
| every keyword used as an identifier is E0005 (§ 4.4) | `local while: u32 = 1;` reports E0005 instead of E0100 |
| `'` starts a char literal (§ 4.9) | `'` reports E0007 instead of E0001 |
| the initial value of a global is a constant expression checked by the typing rules (§ 8.18) | `global g: u32 = 1 + 1;` reports E0409 instead of E0100 |
| a lone `_` is a token (§ 4.10) | `local _: u32;` reports E0100 instead of E0004 |
| exported functions have no parameter limit (§ 13.6) | E0205 is no longer reported |
| `weasel call` prints NaN as `nan` (§ 14.5) | output of `weasel call` for a NaN float |
| integer arithmetic traps on overflow (§ 10.2) | `255 + 1` on `u8` traps instead of giving `0`; wrapping is written `u8.wrapping_add(255, 1)` |
| integer conversions trap when the value does not fit (§ 10.5) | `u8(n)` with `n: u32` = 300 traps instead of giving `44`; the low-order bits are `u8.wrapping_from(n)` |
| float to integer conversions trap on NaN and out of range (§ 10.5) | `u8(x)` with `x: f64` = 300.7 traps instead of giving `255` |
| a call used as a statement must call a function without a return value (until 0.5, § 1.10) | `incr();` with `func incr() -> u32` reports E0413; write `_ = incr();` |

A component whose exported functions only use scalar types is built exactly as in v0.1: it has no linear memory and no `realloc` (§ 13.6).

## 1.8 Changes from v0.2

Version 0.3 accepts every valid v0.2 program with the same behavior. `resource`, `constructor`, `static`, `self`, `borrow` and `import` were already reserved (§ 4.4) and become keywords; the only other changes are:

| Change | Effect on v0.2 programs |
|---|---|
| `?` is a token (§ 4.10) | `?` reports a syntax error (E0100) where it is misplaced, instead of E0001 |
| `wasl.toml` accepts a `[dependencies]` table (§ 3.2) | a manifest with `[dependencies]` is no longer E0200 |

## 1.9 Changes from v0.3

Version 0.4 accepts every valid v0.3 program with the same behavior. `async`, `stream` and `future` were already reserved (§ 4.4) and become a keyword and type names; the only other changes are:

| Change | Effect on v0.3 programs |
|---|---|
| `task` is a keyword, `stream_writer` and `future_writer` are type names, `subtask` is reserved (§ 4.4) | an identifier named `task`, `stream_writer`, `future_writer` or `subtask` is now E0005 |
| `await` has its own error (§ 4.4) | `await` reports E0101 instead of E0005 |
| a tuple can hold handles, as a temporary (§ 7.12) | a function that returns `tuple<carts::cart, u32>` is no longer E0220 |
| `local` can destructure a tuple (§ 9.4) | `local (a: u32, b: u32) = t;` is no longer E0100 |
| the `async` functions of a foreign interface can be used (§ 2.5) | they are no longer E0215 |
| the items of a foreign interface that use `stream<T>` or `future<T>` can be used (§ 2.5) | they are no longer E0215 |
| a foreign interface with `async` functions can be exported (§ 13.12) | `export wasi:cli/run { … }` for WASI 0.3 is no longer E0218 |
| a WIT name with uppercase letters is written in lowercase (§ 13.2) | `error_code::dns_timeout` names `DNS-timeout`, which was E0215 |

All of these changes are available.

## 1.10 Changes from v0.4

*Specified, not available yet.* Version 0.5 changes the syntax of every program: the compiler of 0.5 rejects most programs of 0.4, with errors that say how to write them. There are no users to spare yet, and the language is easier to read for it. `weasel fmt` does not convert old programs; the examples of the repository are rewritten.

| Change | Effect on v0.4 programs |
|---|---|
| built-in functions and constructors are written with `.` (§ 5.3) | `list::len(xs)`, `result::ok(x)`, `option::none`, `task::return(v)` report E0100; write `list.len(xs)`, `result.ok(x)`, `option.none`, `task.return(v)` |
| a file names the interfaces of other packages it uses with `use` (§ 6.6) | `stdout::get_stdout()` without `use wasi:cli/stdout;` in its file reports E0300 |
| a call statement discards its value (§ 9.6) | `incr();` is valid; `_ = incr();` reports E0100 |
| `for`, `in`, `type`, `use` and `as` are keywords (§ 4.4) | an identifier named `in` reports E0005 |
| a body can be a simple statement without braces (§ 5.3) | `if (n > 0) return n;` is valid |
| a local can be declared without type (§ 9.4) | `local x = 5;` is valid, and `x` is an `s32` |
| `==` compares records, tuples, lists, variants, options and results (§ 8.3) | `inv == other` is valid when the type has no `mut` field and no handle |
| `///` starts a doc comment (§ 4.2) | a `///` comment before a declaration is copied into the WIT |
| the standard library is a dependency of every project (§ 3.5) | `"wasl:std"` in `[dependencies]` reports E0200 |
