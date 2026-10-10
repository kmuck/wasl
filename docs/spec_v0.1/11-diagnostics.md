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

## 11.2 First error only

Compilation **stops at the first error**. Which error is first is fully determined, so that a given program always produces the same diagnostic. The package is analyzed in this order:

1. **Lexing and parsing**, file by file in the order of § 3.3. Each file is entirely tokenized (lexical errors, E0001 to E0005), then parsed (syntax errors, E0100), before the next file.
2. **Top-level names**, in file order then text order: duplicates (E0201).
3. **Functions of the interfaces**, interface by interface: duplicates (E0202).
4. **Worlds**, in declaration order: exports (E0203, E0204).
5. **Exported functions**: parameter limit (E0205).
6. **Initial values of the globals**, in declaration order.
7. **Function bodies**, interface by interface, function by function, statement by statement. Within an expression, operands are analyzed from left to right, except that the right operand of a binary operator is analyzed first when the left one is literal-only (§ 8.2).

Reporting several errors per compilation is planned for a later version.

## 11.3 Error codes

Codes never change meaning from one version to the next.

| Code | Error | Example |
|---|---|---|
| E0001 | invalid character | `local é: u32;`, `a @ b` |
| E0002 | invalid integer literal | `007`, `0x1F`, `1e10` |
| E0003 | invalid float literal | `1.`, `.5`, `1.5e3` |
| E0004 | invalid identifier | `Fib`, `_x`, `a__b`, `a_1` |
| E0005 | reserved word used as an identifier | `local record: u32 = 1;` |
| E0100 | syntax error | `local x = 5;`, `if n > 0 { }`, `x + 1;` |
| E0200 | invalid manifest | missing field in `wasl.toml`, invalid package name |
| E0201 | name declared twice at the top level | `interface a { }` and `global a: u32 = 0;` |
| E0202 | function declared twice in an interface | two `func f()` in `interface a` |
| E0203 | world exporting an unknown interface | `world w { export nope; }` |
| E0204 | interface exported twice by a world | `world w { export a; export a; }` |
| E0205 | exported function with more than 16 parameters | |
| E0300 | unknown name | `return y;` with no `y` in scope |
| E0301 | unknown function | `nope();` |
| E0302 | wrong number of arguments | `fib(1, 2)` for `fib(n: u32)` |
| E0303 | parameter or local with the name of a global | `local pi: f64 = 1.0;` with `global pi` |
| E0304 | parameter or local already declared | `local n: u32 = 1;` with a parameter `n` |
| E0400 | mismatched types | `local y: u64 = n;` with `n: u32` |
| E0401 | literal out of range | `local x: u8 = 256;` |
| E0402 | cannot infer the type of a literal | `if (1 < 2) { }`, `u64(5)` |
| E0403 | operator not applicable to this type | `-n` with `n: u32`, `x % y` with floats |
| E0404 | invalid conversion | `u32(b)` with `b: bool` |
| E0405 | non-boolean condition | `if (n) { }` with `n: u32` |
| E0500 | assignment to a global that is not `mut` | `pi = 3.0;` |
| E0501 | `break` or `continue` outside a `while` | |
| E0502 | `return` without a value in a function that returns one | `return;` in `func f() -> u32` |
| E0503 | `return` with a value in a function without a return type | `return 1;` in `func f()` |
| E0504 | a path does not end with `return` | § 8.8 |

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
