# v2.8 Diagnostics

The format (§ 11.1) and the first-error rule (§ 11.2) are unchanged. Codes keep their meaning from v0.1 (§ 11.3); v0.2 adds new ones and retires one.

## v2.8.1 Order of analysis

The order of § 11.2 becomes:

1. **Lexing and parsing**, file by file (E0001 to E0006, E0100).
2. **Top-level names**, in file order then text order: duplicates (E0201).
3. **Items of the interfaces**, interface by interface, in declaration order: functions declared twice (E0202), other names declared twice (E0206), types with the name of a top-level declaration (E0210), fields and cases declared twice (E0207).
4. **Types**, interface by interface, type by type, in declaration order: unknown types in fields, payloads and signatures (E0305), types without a value (E0211).
5. **Worlds**, in declaration order: exports (E0203, E0204).
6. **Exported functions**, world by world, function by function: recursive types (E0208), types of interfaces with functions (E0209).
7. **Initial values of the globals**, in declaration order (E0409 and the typing errors).
8. **Function bodies**, as in § 11.2.

## v2.8.2 New error codes

| Code | Error | Example |
|---|---|---|
| E0006 | invalid string literal | `"abc` (not closed), `"\a"`, `"\u{D800}"`, a line break inside the literal |
| E0206 | name declared twice in an interface | `record line { … }` and `func line()` in the same interface |
| E0207 | field or case declared twice | `record r { a: u32, a: u64 }`, `enum e { x, x }` |
| E0208 | recursive type in the signature of an exported function | `func root() -> node` with `record node { children: list<node> }` |
| E0209 | exported function using a type of an interface that has functions and is not exported by the world | § v2.7.1 |
| E0210 | type with the name of a top-level declaration | `record math { … }` with `interface math` |
| E0211 | type without a value: a cycle that does not go through `option` or `list` | `record a { next: a }` |
| E0305 | unknown type | `local x: nope = …;`, `local x: invoices::fib = …;` with a function `fib` |
| E0306 | unknown field | `inv.nope`, `n.x` with `n: u32` |
| E0307 | unknown case | `status::lost`, `case lost { }`, `invoices::line::x` with a record `line` |
| E0406 | record literal with a missing, repeated or misplaced field | `line { quantity: 1, label: "pen", unit_price: 5 }` |
| E0407 | payload where the case has none, or no payload where it has one | `status::paid()`, `discount::percent`, `case none(x) { }` |
| E0408 | cannot infer the type of a value | `match (option::none) { … }`, `local n: u32 = list::len([]);` |
| E0409 | initial value of a global that is not a constant expression | `global g: u32 = 1 + 1;`, `global s: string = "a" + "b";` |
| E0410 | local of a heap type or of an enum without initial value | `local s: string;` |
| E0411 | `match` on a type that is not an enum, a variant, an option or a result | `match (n) { … }` with `n: u32` |
| E0505 | assignment to a field that is not `mut`, or to an element of a `list<T>` | `inv.number = 1;`, `xs[0] = 1;` with `xs: list<u8>` |
| E0506 | `match` that does not cover every case | `match (s) { case draft { } }` with three cases and no `else` |
| E0507 | case listed twice in a `match`, or `else` that covers no case | two `case none`, `else` after all the cases |

Errors that existing codes already describe keep them: a wrong type for a field value, an argument or an index is E0400; indexing something that is not a list, or `<` on strings, is E0403; an unknown interface in a path is E0300.

## v2.8.3 Retired code

E0205 (exported function with more than 16 parameters) is no longer reported: parameters beyond 16 flat values go through linear memory (§ v2.7.3). The code is not reused.
