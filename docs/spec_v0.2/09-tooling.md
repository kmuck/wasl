# v2.9 Tooling

The commands of chapter 14 are unchanged, except for the values that `weasel call` reads and prints. The `waslc` API is unchanged: a v0.2 compiler accepts more programs and produces components that use WasmGC.

## v2.9.1 Values in `weasel call`

`weasel call` reads its arguments and prints its result in **WAVE**, the *WebAssembly Value Encoding* of the Bytecode Alliance: the standard text format for component values, used by `wasmtime run --invoke` and `wasm-tools`. Values are written with **WIT names**, in `kebab-case`, because they are values of the component, not of the source code.

| Type | WAVE | Example |
|---|---|---|
| `bool`, integers | as in § 14.2 | `true`, `-5`, `42` |
| floats | as in § 14.2, with `nan` for NaN | `1.5`, `-inf`, `nan` |
| `char` | a char literal | `'A'`, `'\n'` |
| `string` | a quoted string with the escapes of § v2.3.1 | `"ACME"`, `"a\"b"` |
| record | fields in braces | `{label: "pen", quantity: 3, unit-price: 150}` |
| enum | the case | `paid`, `empty-customer` |
| variant | the case, with its payload in parentheses | `percent(10)`, `fixed(500)` |
| `list<T>` | elements in brackets | `[1, 2, 3]`, `[]` |
| `tuple<…>` | elements in parentheses | `(2, 1170)` |
| flags | the flags of the set in braces | `{email, urgent}`, `{}` |
| `option<T>` | `some(…)` or `none` | `some(percent(10))`, `none` |
| `result<…>` | `ok(…)`, `err(…)`, or `ok` / `err` without payload | `ok(1170)`, `err(no-lines)` |

- Each argument is one shell argument; a value that contains spaces or quotes is quoted for the shell: `weasel call invoices::summary '{number: 7, …}'`.
- The result is printed in WAVE on one line. Every scalar is printed as in v0.1, except NaN, written `nan` as WAVE requires (§ v2.1.3).
- A `result` is a value like any other: `err(…)` is printed on standard output with the exit code `0`. Only a trap fails the command (§ 14.2).
- An argument that is not a valid WAVE value of its type is an error: ``error: invalid value `{number: 1}` for parameter `inv` of type `invoice`: missing field `customer` ``.

## v2.9.2 Runtime configuration

`weasel` creates the wasmtime engine with the WasmGC proposal and its prerequisites enabled (§ v2.6), for every component, so that the same configuration runs v0.1 and v0.2 components.
