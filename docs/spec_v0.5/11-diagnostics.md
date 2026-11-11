# 11. Diagnostics

## 11.1 Format

A compilation error is reported on one line:

```
<file>:<line>:<column>: error[<code>]: <message>
```

```
src/math.wasl:3:12: error[E0400]: mismatched types: expected `u64`, found `u32`
```

- `<file>` is the path of the source file relative to the project root, with `/` separators (§ 3.3).
- `<line>` and `<column>` start at 1. The column counts **bytes** from the start of the line.
- The location is the start of the faulty construct (the token, the name or the expression).
- `<code>` is a stable code (§ 11.3); `<message>` is an English sentence that explains the error and, when possible, how to fix it.
- An error that is not located in a source file omits the location: `error[<code>]: <message>`.

## 11.2 Which errors are reported

A compilation reports **several errors**, but never an error that another one may have caused. The rule follows the steps below:

- **Syntax**: each file reports its first lexical or syntax error, and every file is parsed. A package with a syntax error is not analyzed further: the declarations missing from the broken file would cause false errors.
- **Declarations** (steps 2 to 8): the first error stops the compilation. Every function body depends on the declarations, so a wrong declaration would be reported again in each body that uses it.
- **Function bodies** (step 9): each function reports its first error, and the analysis goes on with the next function. A function is analyzed alone, so its error cannot cause an error in another function.
- **Reachability** (step 10): every call that a world reaches without importing its interface, every stream or future type that it reaches without carrying it in its WIT, and every call it reaches to an entry point compiled into the same component, is reported once.

The errors are reported in the order of the steps, which is fully determined, so that a given program always produces the same diagnostics. A tool may show only the first ones: `weasel` shows ten (§ 14.1). The package is analyzed in this order:

1. **Lexing and parsing**, file by file in the order of § 3.3. Each file is entirely tokenized (lexical errors, E0001 to E0007), then parsed (syntax errors, E0100, E0101), before the next file.
2. **Top-level names**, in file order then text order: duplicates (E0201).
3. **Foreign interfaces**: the packages named by the `import` and `export` items of the worlds, in declaration order, must be dependencies, and the interfaces must exist in them (E0215); then the last names of all the interfaces that code can name, against each other and against the top-level names of the package (E0217).
4. **Items of the interfaces**, interface by interface, in declaration order: functions declared twice (E0202), other names declared twice (E0206), types with the name of a top-level declaration (E0210), fields, cases and flags declared twice (E0207), flags types with more than 32 flags (E0212).
5. **Types**, interface by interface, type by type, in declaration order: unknown types in fields, payloads and signatures (E0305), types without a value (E0211), `borrow` misused (E0219), handle types in a forbidden position (E0220).
6. **Worlds**, in declaration order, item by item: imports and exports (E0203, E0204, E0216), then the blocks of foreign exports against the WIT of their interface (E0218).
7. **Exported interfaces**, then the imported interfaces of the package, world by world, interface by interface: the signatures of their functions, then their declared types, in declaration order: recursive types (E0208), types of interfaces with functions (E0209), mutable types (E0214), writable ends of streams and futures (E0222); then cycles of `use` between the interfaces that appear in the WIT (E0213).
8. **Initial values of the globals**, in declaration order (E0409 and the typing errors).
9. **Function bodies**, interface by interface, function by function, then the blocks of foreign exports, world by world, statement by statement. Within an expression, operands are analyzed from left to right, except that the right operand of a binary operator is analyzed first when the left one is literal-only (§ 8.2).

10. **Reachability**, world by world, among the functions reachable from its exports: the calls to a function of an interface the world does not import (E0221), the stream and future types that no function of its WIT carries (E0223), and the calls to a function that contains `task::return` compiled into the same component (E0419).

Errors of the dependencies (§ 3.5) are reported before step 1, with the location of the dependency: `wasl.toml` for its declaration, the files of a path dependency for its own errors. The first one stops the compilation.

```
src/orders.wasl:7:12: error[E0300]: unknown name `totl`
src/orders.wasl:19:12: error[E0400]: mismatched types: expected `u32`, found `u64`
```

## 11.3 Error codes

Codes never change meaning from one version to the next. E0101, E0222, E0223 and E0416 to E0419 belong to 0.4.

| Code | Error | Example |
|---|---|---|
| E0001 | invalid character | `local é: u32;`, `a @ b` |
| E0002 | invalid integer literal | `007`, `0x1F`, `1e10` |
| E0003 | invalid float literal | `1.`, `.5`, `1.5e3` |
| E0004 | invalid identifier | `Fib`, `_x`, `a__b`, `a_1` |
| E0005 | reserved word used as an identifier | `local record: u32 = 1;` |
| E0006 | invalid string literal | `"abc` (not closed), `"\a"`, `"\u{D800}"`, a line break inside the literal |
| E0007 | invalid char literal | `''`, `'ab'`, `'a` (not closed), `'\u{D800}'` |
| E0100 | syntax error | `local x = 5;`, `if n > 0 { }`, `x + 1;`, `_ = x;` |
| E0101 | `await` written: a call to an async function waits by itself (§ 4.4) | `client::send(req).await?`, `await f(x)` |
| E0200 | invalid manifest | missing field in `wasl.toml`, invalid package name |
| E0201 | name declared twice at the top level | `interface a { }` and `global a: u32 = 0;` |
| E0202 | function declared twice in an interface | two `func f()` in `interface a` |
| E0203 | world exporting an unknown interface | `world w { export nope; }` |
| E0204 | interface exported twice by a world | `world w { export a; export a; }` |
| E0205 | *retired* (§ 11.6) | |
| E0206 | name declared twice in an interface | `record line { … }` and `func line()` in the same interface |
| E0207 | field, case or flag declared twice | `record r { a: u32, a: u64 }`, `enum e { x, x }` |
| E0208 | recursive type in the signature of an exported function or declared in an exported interface | `func root() -> node` with `record node { children: list<node> }` |
| E0209 | exported function or type of an exported interface using a type of an interface that has functions and is neither exported nor imported by the world | § 13.1 |
| E0210 | type with the name of a top-level declaration | `record math { … }` with `interface math` |
| E0211 | type without a value: no value of it can be built (§ 7.9) | `record a { next: a }`, `variant v { a(v) }` |
| E0212 | flags type with more than 32 flags | |
| E0213 | interfaces of the WIT that use each other's types in a cycle | `record r { s: b::s }` in `a` and `func f(r: a::r)` in `b`, both exported (§ 13.1) |
| E0214 | `mut` field or `list<mut T>` in the signature of an exported function or in a type declared in an exported interface (§ 7.11) | `func sort(xs: list<mut u32>)`, `record cell { v: mut u32 }` in an exported interface |
| E0215 | unknown foreign interface, package that is not a dependency, or item of a foreign interface that WaSL cannot use (§ 2.4.2, § 2.5, § 3.5) | `import wasi:cli/stdout;` without `"wasi:cli"` in `[dependencies]`, `import wasi:cli/nope;` |
| E0216 | interface imported twice by a world, or imported and exported by it | `world w { import a; export a; }` |
| E0217 | two interfaces that code can name have the same last name, or an interface of another package has the name of a world or a global (§ 2.5) | `import example:catalog/carts;` with an `interface carts` in the package |
| E0218 | foreign export block that does not match the WIT of its interface: a missing, extra or different function, including a `func` where the WIT has an `async func` or the reverse, or an interface whose resources WaSL cannot implement yet | `export wasi:cli/run { func run() -> u32 { … } }`, `func run() -> result` for WASI 0.3 |
| E0219 | `borrow<T>` with a `T` that is not a resource, outside the type of a parameter, or a `borrow` parameter moved (§ 7.12, § 9.9) | `func f() -> borrow<cart>`, `borrow<u32>`, `return c;` with `c: borrow<cart>` |
| E0220 | handle type in a record, list, field or global, a local, parameter or binding whose type contains one without being one, an element read from a tuple that holds one, or a stream whose elements contain one (§ 7.12, § 7.13) | `record r { c: carts::cart }`, `local o: option<carts::cart> = …;`, `types::response::new(…).0`, `stream<carts::cart>` |
| E0221 | function reachable from the exports of a world that calls a foreign interface the world does not import (§ 2.5) | `stdout::get_stdout()` reached from a world without `import wasi:cli/stdout;` |
| E0222 | `stream_writer<T>` or `future_writer<T>` in the signature of an exported function or in a type declared in an exported interface (§ 7.11, § 7.13) | `func pipe() -> stream_writer<u8>` in an exported interface |
| E0223 | function reachable from the exports of a world that uses a stream or future type that no function of the world's WIT carries (§ 7.13) | `stream::new()` for a `stream<string>` in a world whose functions never name `stream<string>` |
| E0300 | unknown name | `return y;` with no `y` in scope |
| E0301 | unknown function | `nope();` |
| E0302 | wrong number of arguments | `fib(1, 2)` for `fib(n: u32)` |
| E0303 | parameter or local with the name of a global | `local pi: f64 = 1.0;` with `global pi` |
| E0304 | parameter or local already declared | `local n: u32 = 1;` with a parameter `n` |
| E0305 | unknown type | `local x: nope = …;`, `local x: invoices::fib = …;` with a function `fib` |
| E0306 | unknown field, flag or tuple element | `inv.nope`, `n.x` with `n: u32`, `t.2` on a `tuple<u32, u32>` |
| E0307 | unknown case | `status::lost`, `case lost { }`, `invoices::line::x` with a record `line` |
| E0308 | unknown method or static function, or constructor of a resource that has none (§ 8.20) | `c.nope()`, `carts::cart::nope()` |
| E0309 | field or literal of a resource outside its interface (§ 7.12) | `c.items` or `carts::cart { … }` in another interface |
| E0400 | mismatched types | `local y: u64 = n;` with `n: u32` |
| E0401 | literal out of range | `local x: u8 = 256;` |
| E0402 | cannot infer the type of a literal | `if (1 < 2) { }`, `u64(5)` |
| E0403 | operator not applicable to this type | `-n` with `n: u32`, `x % y` with floats |
| E0404 | invalid conversion | `u32(b)` with `b: bool` |
| E0405 | non-boolean condition | `if (n) { }` with `n: u32` |
| E0406 | record literal with a missing, repeated or misplaced field | `line { quantity: 1, label: "pen", unit_price: 5 }` |
| E0407 | payload where the case has none, or no payload where it has one | `status::paid()`, `discount::percent`, `case none(x) { }` |
| E0408 | cannot infer the type of a value | `match (option::none) { … }`, `local n: u32 = list::len([]);` |
| E0409 | initial value of a global that is not a constant expression | `global g: u32 = 1 + 1;`, `global s: string = "a" + "b";` |
| E0410 | local without initial value whose type has no default value: a type other than a scalar or an `option` (§ 9.4) | `local s: string;`, `local t: tuple<u32, u32>;` |
| E0411 | `match` on a type that is not an enum, a variant, an option or a result | `match (n) { … }` with `n: u32` |
| E0412 | flags literal with a repeated or misplaced flag | `delivery { urgent, email }`, `delivery { paper, paper }` |
| E0413 | call used as a statement whose function returns a value (§ 9.6) | `save(inv);` with `func save(…) -> result<_, e>`, `string::len(s);` |
| E0414 | `?` on a value that is not a `result` or an `option` (§ 8.19) | `n?` with `n: u32` |
| E0415 | `?` in a function whose return type cannot receive the error: not a `result` with the same error type, or not an `option` | `prices::of(sku)?` in `func f() -> u64`, `o?` with `o: option<u32>` in a function that returns a `result` |
| E0416 | async call in a function that is not async (§ 8.21) | `client::send(req)` or `stream::write(tx, xs)` in a `func` without `async` |
| E0417 | `task::return` in a function that is not async or has no return type, inside an `if`, a `while` or a `match`, or written twice (§ 8.21) | `task::return(r);` inside an `if` |
| E0418 | `return e;`, `?` or a `borrow` parameter after `task::return` (§ 8.21) | `return result::err(e);` after `task::return(result::ok(r));` |
| E0419 | function reachable from the exports of a world that calls a function containing `task::return` compiled into the same component (§ 8.21) | `handle(req)` called by another function of the package |
| E0500 | assignment to a global that is not `mut` | `pi = 3.0;` |
| E0501 | `break` or `continue` outside a `while` | |
| E0502 | `return` without a value in a function that returns one | `return;` in `func f() -> u32` |
| E0503 | `return` with a value in a function without a return type | `return 1;` in `func f()` |
| E0504 | a path does not end with `return` | § 8.8 |
| E0505 | assignment to a field that is not `mut`, to an element of a `list<T>` or of a tuple, or to a flag | `inv.number = 1;`, `xs[0] = 1;` with `xs: list<u8>`, `t.0 = 1;`, `d.paper = true;` |
| E0506 | `match` that does not cover every case | `match (s) { case draft { } }` with three cases and no `else` |
| E0507 | case listed twice in a `match`, or `else` that covers no case | two `case none`, `else` after all the cases |

Some errors share a code with errors of the same kind: a wrong type for a field value, an argument or an index is E0400; indexing something that is not a list, or `<` on strings, is E0403; an unknown interface in a path is E0300; an unknown built-in function of an integer type, such as `u32::wrapping_neg`, is E0301; `T::wrapping_from` of a float, a `bool` or a `char` is E0404; `_ = f();` on a function without a return value is E0400, and `_ =` followed by anything but a call is E0100 (§ 9.6). A destructuring of a value that is not a tuple, or with the wrong number of positions, is E0400 (§ 9.4); `stream::new()` and `future::new()` without an expected type are E0408 (§ 8.21).

Codes `E9xxx` are reserved for errors that do not come from the source code:

| Code | Error |
|---|---|
| E9000 | input/output error (reserved) |
| E9001 | unknown world requested from the compiler (§ 14.3) |
| E9999 | internal compiler error: a bug, to be reported |

## 11.4 Manifest errors

Errors of `wasl.toml` (§ 3.2) use code E0200 and the file name `wasl.toml` instead of a source file:

```
wasl.toml:5:1: error[E0200]: unknown field `author`, expected one of `namespace`, `name`, `version`
wasl.toml: error[E0200]: invalid package name `My_Calc`: expected lowercase words separated by `-`, each starting with a letter
```

The first form is used for TOML syntax and structure errors, located in the file; the second for invalid values.

## 11.5 Tool errors

Errors of the build tool that are not about the code (no project in the current directory, unreadable file, unknown function in `weasel call`, trap…) are reported without code (§ 14.1):

```
error: could not find `wasl.toml` in the current directory: weasel must be run from the root of a WaSL project
error: `math::div` trapped: integer divide by zero
```

## 11.6 Retired codes

A retired code is no longer reported and is never reused.

| Code | Error | Retired in | Why |
|---|---|---|---|
| E0205 | exported function with more than 16 parameters | v0.2 | parameters beyond 16 flat values go through linear memory (§ 13.6) |
