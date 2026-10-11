# v2.4 Typing

This chapter extends chapters 8 and 9 to the new types. The general rule of § 8.1 is unchanged: every expression has one type, determined at compile time, and every value has exactly the type its context expects, with the single exception of § v2.2.6.

## v2.4.1 Expected types

The expected type of § 8.2 is extended to the new contexts:

| Context | Expected type |
|---|---|
| value of a field in a record literal | the type of the field |
| element of a list literal | the element type of the expected list type, if any |
| payload of a constructor (`option::some(e)`, `shape::circle(e)`, `result::ok(e)`) | the payload type, when the constructor's type is known |
| index `xs[e]` | `u32` |
| `x.f = e;`, `xs[i] = e;` | the type of the field or of the element |
| expression of `match` | none |
| operand of `==`, `!=`, `+` | the rules of § 8.2 for binary operators |

String literals, `true` and `false` are never literal-only (§ 8.2): a string literal always has type `string`.

**Values whose type comes from the context.** `option::none`, `result::ok(…)`, `result::err(…)`, `result::ok`, `result::err` and the empty list `[]` cannot determine their own type: they take the expected type. Where there is none, or where the expected type is not of the right kind, it is an error: E0408 when there is no expected type (`match (option::none) { … }`), E0400 when the expected type is another type (`local x: u32 = option::none;`).

## v2.4.2 Paths

A path `a::b` or `a::b::c`, as an expression or before a call, is resolved by the kind of its first name:

| First name | Meaning | Examples |
|---|---|---|
| `string`, `list` | a built-in function (§ v2.4.11) | `string::len(s)`, `list::filled(3, 0)` |
| `option`, `result` | a constructor of the built-in type | `option::none`, `result::ok(sum)` |
| a type of the current interface | a case of that enum or variant | `status::paid`, `discount::fixed(500)` |
| an interface of the package, followed by one name | a function of that interface (§ 9.1) | `pricing::apply(sum, d)` |
| an interface of the package, followed by two names | a case of a type of that interface | `invoices::status::paid` |

An unknown first name is E0300 (§ 9.1); a path that names a type that is not an enum or a variant, or an unknown case, is E0307; a case written with the wrong payload is E0407 (§ v2.4.7).

## v2.4.3 String literals

A string literal has type `string`. Its value is the UTF-8 encoding of its characters, after replacing the escapes (§ v2.3.1).

## v2.4.4 Record literals

```wasl
line { label: "pen", quantity: 3, unit_price: 150 }
invoices::line { label: "pen", quantity: 3, unit_price: 150 }
```

- The name before `{` designates a record type (§ v2.2.4); anything else is E0305.
- Every field of the record appears **exactly once, in declaration order** (E0406). Requiring the order keeps one way to write each literal and makes the evaluation order the declaration order (§ v2.5.2).
- Each value has the type of its field; `mut` fields are initialized like the others.
- The literal has the record type and creates a new record.

## v2.4.5 Field access

`e.f`, where `e` has a record type and `f` is one of its fields, has the type of the field. If `e` is not a record, or the record has no field `f`, it is an error (E0306).

## v2.4.6 Lists

**Literals.** `[e1, e2, …]` creates a new list with these elements.

- With an expected type `list<T>` or `list<mut T>`, every element has type `T` and the literal has the expected type.
- Without an expected type, the element type is the type of the first element that is not literal-only (§ 8.2), and the literal has type `list<T>`; if every element is literal-only, or the list is empty, it is E0408.

**Indexing.** `e[i]`, where `e` has type `list<T>` or `list<mut T>` and `i` has type `u32`, has type `T`. Indexing anything else is E0403. An index outside the list traps (§ v2.5.5).

Strings cannot be indexed: their elements are bytes of an encoding, not characters. `string::bytes(s)[i]` reads the byte at position `i`.

## v2.4.7 Enum cases and variant constructors

- `e::c`, where `e` is an enum and `c` one of its cases, has type `e`.
- `v::c(x)`, where `v` is a variant and `c` a case with a payload of type `P`, has type `v`; `x` has type `P`.
- `v::c`, where `c` is a case without payload, has type `v`.
- Writing a payload for a case that has none, or omitting it for a case that has one, is E0407: `status::paid()`, `discount::percent`.

The type is always written before the case, even when the expected type would determine it: `return status::paid;`, never `return paid;`.

## v2.4.8 `option` and `result` constructors

| Expression | Type |
|---|---|
| `option::some(x)` | the expected `option<T>`, with `x: T`; without an expected type, `option<T>` where `T` is the type of `x` (E0408 if `x` is literal-only) |
| `option::none` | the expected `option<T>` (§ v2.4.1) |
| `result::ok(x)`, `result::ok` | the expected `result<…>`, whose `ok` has a payload of the type of `x`, or no payload |
| `result::err(x)`, `result::err` | the expected `result<…>`, whose `err` has a payload of the type of `x`, or no payload |

As for variants, writing a payload where the expected form has none, or omitting it where it has one, is E0407: with `result<_, e>`, write `result::ok`, not `result::ok()`.

## v2.4.9 `match`

```wasl
match (inv.discount) {
  case some(d) {
    return pricing::apply(sum, d);
  }
  case none {
    return result::ok(sum);
  }
}
```

- The expression in parentheses is evaluated once. Its type must be a case type: an enum, a variant, an option or a result (E0411).
- Each `case c` names a case of that type (E0307), written without its type: `case paid`, `case percent(p)`, `case some(d)`, `case err(e)`.
- **Bindings.** For a case with a payload, `case c(x)` declares a local `x` holding the payload, visible in the block of the case only. It follows the rules of locals: it can be assigned, and it cannot have the name of a global (E0303) or of a visible local (E0304). `case c` without a name ignores the payload. Writing a name for a case without payload is E0407.
- **Exhaustiveness.** Every case of the type is listed exactly once, or the `match` ends with an `else` block that runs for every case not listed (E0506). A case listed twice, or an `else` when every case is already listed, is E0507.
- Exactly one block runs: the block of the matching case, or the `else` block. After it, execution continues after the `match`.
- `break` and `continue` inside a `match` act on the enclosing `while` (§ 9.7), as inside an `if`.

## v2.4.10 Operators

The table of § 8.3 is extended:

| Operators | Operand type | Result type |
|---|---|---|
| `==` `!=` | enum, same type | `bool` |
| `==` `!=` | `string` | `bool` |
| `+` | `string` | `string` |

- Two enum values are equal when they are the same case. Two strings are equal when they have the same bytes.
- `+` on strings creates a new string, the concatenation of the two operands.
- Every other operator on a heap type or an enum is E0403. In particular, records, variants, lists, options and results cannot be compared with `==` (§ v2.11): compare their fields, or use `match`.
- `<`, `<=`, `>` and `>=` do not apply to strings; ordering strings is the job of a library.

## v2.4.11 Built-in functions

Six functions are part of the language. They are called like functions of an interface, with the name of the type as path; they are not values and cannot be redefined.

| Function | Type | Result |
|---|---|---|
| `string::len(s)` | `(string) -> u32` | the number of bytes of `s` |
| `string::bytes(s)` | `(string) -> list<u8>` | the UTF-8 bytes of `s` |
| `string::from_bytes(b)` | `(list<u8>) -> option<string>` | a new string with the bytes of `b` if they are valid UTF-8, `option::none` otherwise |
| `list::len(xs)` | `(list<T>) -> u32` | the number of elements of `xs` |
| `list::filled(n, x)` | `(u32, T) -> list<mut T>` | a new list of `n` elements, all equal to `x` |
| `list::copy(xs)` | `(list<T>) -> list<mut T>` | a new list with the elements of `xs` |

- `list::len` and `list::copy` accept a `list<mut T>` through § v2.2.6; `string::from_bytes` accepts a `list<mut u8>`.
- The `T` of `list::filled` is the element type of the expected list type, or else the type of `x` (E0408 if `x` is literal-only and nothing else gives the type): `local d: list<mut u8> = list::filled(3, 0);`.
- `string::bytes` does not copy: the result is a read-only view of the bytes of the string, which never change.
- `list::filled` and `list::copy` are shallow: the elements that are references are shared, not copied (§ v2.5.1).

This set is deliberately minimal: it is what libraries written in WaSL need to build everything else (number formatting, search, slicing, sorting), following § 1.2, principle 7.

## v2.4.12 Globals

A global can have any type. Its initial value is a **constant expression** (amends § 6.4), which Wasm can evaluate when the instance starts:

- a literal, optionally preceded by `-` (as in v0.1), or a string literal;
- an enum case, or a constructor of a variant, `option` or `result`, whose payload is a constant expression;
- a record literal or a list literal whose values are constant expressions.

Anything else, such as an operator, a call, a local or another global, is E0409.

```wasl
global company: string = "Weasel & Co";
global default_line: invoices::line = invoices::line { label: "item", quantity: 1, unit_price: 0 };
global no_discount: option<invoices::discount> = option::none;
```

A global that is not `mut` always designates the same value, but that value can have `mut` fields or be a `list<mut T>`, whose contents can change. Its type is written with a path when it is a declared type (§ v2.2.4).

## v2.4.13 Locals

A local of a **heap type** must have an initial value (E0410): there is no zero value for a string, a record or a list, and no `null` (§ v2.1.2). A local of an enum type without initial value is not allowed either (E0410): no case is more of a default than the others.

## v2.4.14 Assignment

The target of an assignment (§ v2.3.4) is:

- a local, a parameter or a `mut` global, as in § 9.5;
- `e.f`, where `f` is a `mut` field of the record `e`;
- `e[i]`, where `e` has type `list<mut T>`.

Assigning a field that is not `mut`, or an element of a `list<T>`, is E0505. Assigning a field of a record stored in an immutable global is allowed when the field is `mut`: the global designates the record, not its contents.

## v2.4.15 Every path returns

The syntactic check of § 8.8 is extended: a `match` always returns if the block of each of its cases, and its `else` block if present, always returns. A `match` is always exhaustive (§ v2.4.9), so no other path exists.
