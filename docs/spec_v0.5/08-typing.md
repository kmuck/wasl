# 8. Typing

## 8.1 General rule

Every expression has a single type, determined at compile time.

- Both operands of a binary operator have **exactly** the same type.
- A call argument has exactly the type of the corresponding parameter.
- An assigned value has exactly the type of its target.
- The value of `return` has exactly the return type of the function.
- The initial value of a `local` or a `global` has exactly its declared type. A `local` declared without a type takes the type of its initial value (§ 9.4).

Any mismatch is an error (E0400), e.g. `local y: u64 = n;` with `n: u32` (write `u64(n)`). This holds for every type: a `list<mut T>` is not a `list<T>` (§ 7.10), and two records with the same fields are different types.

## 8.2 Literals and expected types

A numeric literal has no type of its own: it takes the **type expected** by its context.

| Context | Expected type |
|---|---|
| `local x: T = <lit>;`, `global x: T = <lit>;` | `T` |
| `local x = e;`, without a type | none; a literal-only `e` is an `s32` when it is an integer, an `f64` when it is a float |
| `x = <lit>;` | the type of `x` |
| argument of a call | the type of the parameter |
| `return <lit>;` | the return type of the function |
| operand of a binary operator | see below |
| value of a field in a record literal | the type of the field |
| element of a list literal | the element type of the expected list type, if any |
| element of `tuple(…)` | the corresponding element type of the expected tuple type, if any |
| payload of a constructor (`option.some(e)`, `shape::circle(e)`, `result.ok(e)`) | the payload type, when the constructor's type is known |
| argument of `T.wrapping_add`, `T.checked_add` and the other arithmetic functions of an integer type `T` (§ 8.17) | `T` |
| index `xs[e]` | `u32` |
| `x.f = e;`, `xs[i] = e;` | the type of the field or of the element |
| expression of `match` | none |
| operand of unary `-`, expression in parentheses | the type expected for the enclosing expression |
| argument of a conversion `T(<lit>)` | none |
| condition of `if` / `while` | none (it must be a `bool`, § 8.9) |

**Literal-only expressions.** An expression is *literal-only* if it is an integer or float literal, or a parenthesized literal-only expression, or unary `-` applied to a literal-only expression, or an arithmetic, bitwise or shift operator (`+ - * / % & | ^ << >>`) applied to two literal-only expressions. `true` and `false`, string and char literals and flags literals are not literal-only: `true` and `false` always have type `bool`, a string literal type `string`, a char literal type `char`.

**Operands of a binary operator.** The type of the operands is determined as follows:

1. if the left operand is not literal-only, it is analyzed first, without expected type, and its type is expected for the right operand;
2. otherwise, if the right operand is not literal-only, it is analyzed first, and its type is expected for the left operand;
3. otherwise (both are literal-only): for an arithmetic, bitwise or shift operator, both operands take the type expected for the whole expression (`local x: u32 = 1 + 2;`); for a comparison there is no expected type, which is an error (E0402, e.g. `if (1 < 2)`).

**Rules.**

- An integer literal can only take an integer type, and a float literal only a float type: `local x: f64 = 1;` is an error (E0400, write `1.0`). A numeric literal where a `bool` is expected is an error (E0400).
- If no type can be determined for a literal, it is an error (E0402): `u64(5)`, `if (1 < 2)`.
- An integer literal must be representable in its type (E0401). A unary `-` applied **directly** to an integer literal forms a negative literal, checked as a whole: `local x: s8 = -128;` is valid; `local x: u8 = 256;`, `local x: s8 = 128;` and `local x: s8 = -(128);` are out of range (in the last one, the `-` is not applied directly to the literal, so `128` is checked alone).
- A negative literal for an unsigned type is an error (E0403): `local x: u32 = -1;`.
- A float literal is rounded to the nearest value of its type. A literal larger than the largest finite value of its type is an error (E0401).

**Values whose type comes from the context.** `option.none`, `result.ok(…)`, `result.err(…)`, `result.ok`, `result.err` and the empty list `[]` cannot determine their own type: they take the expected type. Where there is none, or where the expected type is not of the right kind, it is an error: E0408 when there is no expected type (`match (option.none) { … }`, `local x = [];`), E0400 when the expected type is another type (`local x: u32 = option.none;`).

**A local without type.** `local x = e;` analyzes `e` without an expected type, and `x` takes the type of `e`. The only default is for a literal-only `e`: `local n = 0;` is an `s32`, `local r = 0.5;` an `f64`, the most common types of the literals, as in Rust and Go. Inside a larger expression, a literal still takes its type from the other operand: `local total = price * 2;` has the type of `price`. Only the type of a local is inferred: parameters, return types, fields and globals are always written, since they form the contract of the code and of the WIT.

## 8.3 Operators

| Operators | Operand type | Result type |
|---|---|---|
| `+` | numeric or `string`, same type | same type |
| `-` `*` `/` | numeric, same type | same type |
| `%` | integer, same type | same type |
| `&` `\|` | integer or flags, same type | same type |
| `^` | integer, same type | same type |
| `<<` `>>` | integer, same type | same type |
| `==` `!=` | any type without `mut`, handle or resource (below), same type | `bool` |
| `<` `<=` `>` `>=` | numeric or `char`, same type | `bool` |
| `&&` `\|\|` | `bool` | `bool` |
| unary `-` | signed integer or float | same type |
| `!` | `bool` | `bool` |

Applying an operator to a type outside its column is an error (E0403):

- unary `-` on an unsigned integer or a `bool` (`-n` with `n: u32`);
- `%` on floats (Wasm has no float remainder instruction);
- `&`, `|`, `^`, `<<`, `>>` on floats or `bool`;
- `+`, `-`, `*`, `/`, `<`, `<=`, `>`, `>=` on `bool`;
- `&&`, `||`, `!` on anything other than `bool` (`if (n && b)` with `n: u32`);
- any other operator on a heap type, an enum, a flags type or a `char`. `<`, `<=`, `>` and `>=` do not apply to strings; ordering strings is the job of a library.
- `==` and `!=` on a type that contains, at any depth, a `mut` field, a `list<mut T>`, a resource, a handle type or a writable end. A value that can change, or that has an identity, has no equality that stays true: compare what matters with a function.

On the other types:

- Two enum values are equal when they are the same case; two flags values when they contain the same flags; two chars when they are the same code point, and `<` compares code points. Two strings are equal when they have the same bytes.
- **Structural equality**. Two records are equal when their fields are equal, two tuples when their elements are, two lists when they have the same length and equal elements in the same order, two variants, options or results when they have the same case and equal payloads. Floats inside compare with the `==` of floats, so a record that holds a NaN is not equal to itself. The comparison stops at the first difference. It is a function generated by the compiler for each type that is compared (§ 12.5), since WasmGC has no deep equality; a recursive type gets a recursive function. It needs no identity: two values built separately with the same contents are equal, which is what tests compare (§ 14.2).
- `|` on flags is the union of two sets, `&` their intersection: `(d & delivery { paper, email }) == delivery { }` tells that `d` contains neither.
- `+` on strings creates a new string, the concatenation of the two operands.

The shift amount of `<<` and `>>` has the same type as the shifted value: `x << 3` with `x: u8` shifts by the `u8` literal `3`.

## 8.4 Precedence and associativity

From highest to lowest:

| Level | Operators | Associativity |
|---|---|---|
| 1 | call, conversion, `( … )`, field access `.`, method call `.m( … )`, indexing `[ … ]`, `?` | left |
| 2 | unary `-`, `!` | right (prefix) |
| 3 | `*` `/` `%` | left |
| 4 | `+` `-` | left |
| 5 | `<<` `>>` | left |
| 6 | `&` | left |
| 7 | `^` | left |
| 8 | `\|` | left |
| 9 | `==` `!=` `<` `<=` `>` `>=` | none: `a < b < c` is a syntax error |
| 10 | `&&` | left |
| 11 | `\|\|` | left |

Bitwise operators bind tighter than comparisons, unlike in C: `a & b == c` reads `(a & b) == c`. `+` on strings has the precedence of `+`. Field accesses, method calls, indexes and `?` chain from left to right: `all[i].customer`, `c.last_line()?.sku`.

## 8.5 Conversions

A conversion is written as a call to the name of the target type: `u64(n)`, `f32(x)`, `s8(y)`.

- The argument must not be literal-only (E0402): there is no need to convert a literal, write a value of the target type directly. This is checked first: `u8(5)` and `bool(1)` are E0402.
- The argument must have a numeric type and the target type must be numeric. Converting from or to `bool` is an error (E0404): `u32(true)`, `bool(n)` with `n: u32`.
- `u32(c)` with `c: char` gives the code point of `c`; it is the only conversion from a `char`. A conversion to `char`, or from `char` to any other type, is E0404: not every number is a `char`, so a number becomes a `char` with `char.from_u32` (§ 8.17), which returns an `option` instead of trapping.
- A conversion to the same type is allowed and has no effect.
- The value produced by each conversion is specified in § 10.5: it is exact, or the conversion traps.

## 8.6 Calls

- A call `f(args)` or `i::f(args)` designates a function as described in § 9.1.
- The number of arguments must be the number of parameters (E0302).
- Each argument has the type of its parameter (§ 8.1); literals take that type.
- A call to a function without a return value is not a value: using it in an expression is an error (E0400), e.g. `local x: u32 = reset();`. It can only be used as a statement (§ 9.6).
- A built-in function (§ 8.17) is called with the name of its type and a `.`: `string.len(s)`.
- A function of an interface of another package is called like a function of the package, with the name that the file gives it with `use` as path: `stdout::get_stdout()` (§ 2.5, § 6.6).
- Constructors, methods and static functions of resources are in § 8.20.
- A call to an async function is only allowed in an async function (E0416, § 8.21).

## 8.7 Return

- In a function with return type `T`, every `return` carries an expression of type `T`. A `return;` without value is an error (E0502).
- In a function without a return type, `return;` is allowed and `return <expr>;` is an error (E0503). Reaching the end of the body is an implicit `return;`.
- In an async function that has delivered its result with `task.return`, only `return;` is allowed after it, and its `borrow` parameters cannot be used (E0418, § 8.21).

## 8.8 Every path returns

A function with a return type must end with a `return` on **every** execution path (E0504). The check is syntactic:

- a block *always returns* if one of its statements always returns;
- a `return` always returns;
- an `if` always returns if it has an `else` branch and every branch (including the branches of `else if`s) always returns;
- a `while` never always returns, whatever its condition (`while (true) { return 1; }` does not count);
- a `match` always returns if the block of each of its cases, and its `else` block if present, always returns. A `match` is always exhaustive (§ 8.16), so no other path exists;
- a `task.return(e);` always returns: the result is delivered, and the rest of the body runs after (§ 8.21).

Statements after a statement that always returns are allowed; they never run.

```wasl
func sign(x: s32) -> s32 {
  if (x < 0) {
    return -1;
  } else if (x == 0) {
    return 0;
  } else {
    return 1;
  }
}                       // valid: every branch returns

func half(x: u32) -> u32 {
  if (x > 0) {
    return x / 2;
  }
}                       // invalid (E0504): nothing is returned when x == 0
```

## 8.9 Conditions

The condition of `if` and `while` must have type `bool` (E0405). There is no implicit conversion to `bool`: `if (n)` with `n: u32` is an error; write `if (n != 0)`. A literal-only condition such as `if (1)` is also an error (E0405).

## 8.10 String and char literals

A string literal has type `string`. Its value is the UTF-8 encoding of its characters, after replacing the escapes (§ 4.8).

A char literal has type `char`. Its value is its single Unicode scalar value.

## 8.11 Record literals

```wasl
line { label: "pen", quantity: 3, unit_price: 150 }
invoices::line { label: "pen", quantity: 3, unit_price: 150 }
```

- The name before `{` designates a record type (§ 7.8); anything else is E0305.
- Every field of the record appears **exactly once, in declaration order** (E0406). Requiring the order keeps one way to write each literal and makes the evaluation order the declaration order (§ 10.1).
- Each value has the type of its field; `mut` fields are initialized like the others.
- The literal has the record type and creates a new record.
- A **resource literal** has the same form and the same rules, with the name of a resource: `cart { items: 0, amount: 0 }`. It creates a new object of the resource, and is only valid in the interface that declares the resource (E0309, § 7.12).

## 8.12 Field access

- `e.f`, where `e` has a record type and `f` is one of its fields, has the type of the field.
- `e.f`, where `e` has a flags type and `f` is one of its flags, has type `bool`: `true` if the set contains `f`.
- `e.k`, where `e` has a tuple type and `k` is an integer literal smaller than the number of its elements, has the type of element `k`, counting from 0: `t.0`, `t.1`. When the tuple holds a handle, reading one element would lose the others: `types::response::new(…).0` is E0220, and the tuple is taken apart by a destructuring (§ 9.4, § 7.12).

- `e.f`, where `e` has a resource type or `borrow<R>` and `f` is one of its fields, has the type of the field. It is only valid in the interface that declares the resource (E0309).

Any other field access is an error (E0306): an unknown field or flag, `t.2` on a tuple of two elements, `n.x` with `n: u32`.

## 8.13 Lists and tuples

**Literals.** `[e1, e2, …]` creates a new list with these elements.

- With an expected type `list<T>` or `list<mut T>`, every element has type `T` and the literal has the expected type: the same literal builds an immutable or a mutable list.
- Without an expected type, the element type is the type of the first element that is not literal-only (§ 8.2), and the literal has type `list<T>`; if every element is literal-only, or the list is empty, it is E0408.

**Indexing.** `e[i]`, where `e` has type `list<T>` or `list<mut T>` and `i` has type `u32`, has type `T`. Indexing anything else is E0403. An index outside the list traps (§ 10.7).

Strings cannot be indexed: their elements are bytes of an encoding, not characters. `string.bytes(s)[i]` reads the byte at position `i`, `string.chars(s)[i]` the character at position `i`.

**Tuples.** `tuple(e1, …, en)` creates a new tuple. With an expected type `tuple<T1, …, Tn>`, each `ek` has type `Tk`, and the number of elements must match (E0400). Without an expected type, the tuple has the types of its elements (E0408 if one of them is literal-only).

## 8.14 Enum cases, variant constructors and flags literals

- `e::c`, where `e` is an enum and `c` one of its cases, has type `e`.
- `v::c(x)`, where `v` is a variant and `c` a case with a payload of type `P`, has type `v`; `x` has type `P`.
- `v::c`, where `c` is a case without payload, has type `v`.
- Writing a payload for a case that has none, or omitting it for a case that has one, is E0407: `status::paid()`, `discount::percent`.

The type is always written before the case, even when the expected type would determine it: `return status::paid;`, never `return paid;`.

**Flags literals.** `f { a, b }`, where `f` is a flags type, is the set of the flags listed, written in declaration order, each at most once (E0412); `f { }` is the empty set. An unknown flag is E0307. A flags literal is written with its type, like a record literal: `invoices::delivery { paper }` from another interface.

## 8.15 `option` and `result` constructors

| Expression | Type |
|---|---|
| `option.some(x)` | the expected `option<T>`, with `x: T`; without an expected type, `option<T>` where `T` is the type of `x` (E0408 if `x` is literal-only) |
| `option.none` | the expected `option<T>` (§ 8.2) |
| `result.ok(x)`, `result.ok` | the expected `result<…>`, whose `ok` has a payload of the type of `x`, or no payload |
| `result.err(x)`, `result.err` | the expected `result<…>`, whose `err` has a payload of the type of `x`, or no payload |

As for variants, writing a payload where the expected form has none, or omitting it where it has one, is E0407: with `result<_, e>`, write `result.ok`, not `result.ok()`.

## 8.16 `match`

```wasl
match (inv.discount) {
  case some(d) {
    return pricing::apply(sum, d);
  }
  case none {
    return result.ok(sum);
  }
}
```

- The expression in parentheses is evaluated once. Its type must be a case type: an enum, a variant, an option or a result (E0411).
- Each `case c` names a case of that type (E0307), written without its type: `case paid`, `case percent(p)`, `case some(d)`, `case err(e)`.
- **Bindings.** For a case with a payload, `case c(x)` declares a local `x` holding the payload, visible in the block of the case only. It follows the rules of locals: it can be assigned, and it cannot have the name of a global (E0303) or of a visible local (E0304). `case c` without a name ignores the payload. Writing a name for a case without payload is E0407.
- **Exhaustiveness.** Every case of the type is listed exactly once, or the `match` ends with an `else` block that runs for every case not listed (E0506). A case listed twice, or an `else` when every case is already listed, is E0507.
- Exactly one block runs: the block of the matching case, or the `else` block. After it, execution continues after the `match`.
- `break` and `continue` inside a `match` act on the enclosing `while` or `for` (§ 9.7), as inside an `if`.

## 8.17 Built-in functions

A small set of functions is part of the language. They are called with the name of their type followed by `.`, `list.len(xs)`, which tells them apart from the functions declared in code, always named with `::`; they are not values, cannot be redefined, and are never methods (`xs.len()` is E0100). In this table, `list<T>` in a parameter accepts both a `list<T>` and a `list<mut T>`.

| Function | Type | Result |
|---|---|---|
| `string.len(s)` | `(string) -> u32` | the number of bytes of `s` |
| `string.bytes(s)` | `(string) -> list<u8>` | the UTF-8 bytes of `s` |
| `string.from_bytes(b)` | `(list<u8>) -> option<string>` | a new string with the bytes of `b` if they are valid UTF-8, `option.none` otherwise |
| `string.chars(s)` | `(string) -> list<char>` | a new list with the characters of `s` |
| `string.from_chars(cs)` | `(list<char>) -> string` | a new string with the characters of `cs` |
| `char.from_u32(n)` | `(u32) -> option<char>` | the char whose code point is `n`, `option.none` if `n` is not a Unicode scalar value |
| `list.len(xs)` | `(list<T>) -> u32` | the number of elements of `xs` |
| `list.filled(n, x)` | `(u32, T) -> list<T>` or `list<mut T>` | a new list of `n` elements, all equal to `x` |
| `list.copy(xs)` | `(list<T>) -> list<mut T>` | a new mutable list with the elements of `xs` |
| `list.freeze(xs)` | `(list<mut T>) -> list<T>` | a new immutable list with the elements of `xs` |

- `list.filled` has the expected list type, immutable or mutable; without an expected type, it is a `list<T>`. Its `T` is the element type of the expected list type, or else the type of `x` (E0408 if `x` is literal-only and nothing else gives the type): `local d: list<mut u8> = list.filled(3, 0);`.
- `list.freeze` only accepts a `list<mut T>`: freezing a list that is already immutable would be a useless copy (E0400).
- `string.bytes` does not copy: a string never changes, so its bytes are already an immutable `list<u8>`.
- `list.filled`, `list.copy` and `list.freeze` are shallow: the elements that are references are shared, not copied (§ 10.8).

**Integer functions.** Each integer type `T` (`s8` to `u64`) has functions that compute modulo 2ⁿ, or that report an overflow as a value instead of a trap (§ 10.2):

| Function | Type | Result |
|---|---|---|
| `T.wrapping_add(a, b)`, `T.wrapping_sub(a, b)`, `T.wrapping_mul(a, b)` | `(T, T) -> T` | `a + b`, `a - b`, `a * b` reduced modulo 2ⁿ, never a trap |
| `T.wrapping_neg(a)`, signed `T` only | `(T) -> T` | `-a` reduced modulo 2ⁿ: `s8.wrapping_neg(-128)` is `-128` |
| `T.wrapping_from(x)` | `(integer) -> T` | the low-order bits of `x`, read as a `T`, like the integer conversions of § 10.5: `u8.wrapping_from(n)` is `44` for `n: u32` = 300, `u32.wrapping_from(n)` is `4294967295` for `n: s32` = -1 |
| `T.checked_add(a, b)`, `T.checked_sub(a, b)`, `T.checked_mul(a, b)` | `(T, T) -> option<T>` | `option.some` of the exact result, or `option.none` if it does not fit in `T` |
| `T.to_string(n)` | `(T) -> string` | the decimal form of `n`: digits, preceded by `-` when `n` is negative, without leading zero, sign `+` or separator |
| `T.parse(s)` | `(string) -> option<T>` | the integer written in decimal in `s`, as `T.to_string` writes it, or `option.none` if `s` has any other character, is empty, or gives a value that does not fit in `T` |

- The type before the `.` is the expected type of the arguments, so literals need no conversion: `u32.wrapping_mul(h, 33)`. An argument of another type is E0400.
- `T.wrapping_from(x)` takes an integer of any type. Like a conversion (§ 8.5), its argument must not be literal-only (E0402), and a float, a `bool` or a `char` is E0404.
- `wrapping_neg` does not exist for unsigned types, where unary `-` does not exist either (E0301).
- `T.parse` accepts exactly what `T.to_string` writes: no whitespace, no `+`, no leading zero except `0` itself, and `-` only for a signed `T`. A program that reads looser input trims it first (`text::trim`, chapter 17).
- They are built in rather than written in a library because they exist for every integer type, and the language has no generic functions.

```wasl
func hash(s: string) -> u32 {
  local bytes: list<u8> = string.bytes(s);
  local h: u32 = 5381;
  local i: u32;
  while (i < list.len(bytes)) {
    h = u32.wrapping_add(u32.wrapping_mul(h, 33), u32(bytes[i]));
    i = i + 1;
  }
  return h;
}
```

This set is deliberately minimal: it is what libraries written in WaSL need to build everything else (search, slicing, sorting), following § 1.2, principle 7. The standard library builds on it (chapter 17). The built-in functions of streams and futures are in § 8.21.

## 8.18 Globals

A global can have any type. Its initial value is a **constant expression** (§ 6.4), which Wasm can evaluate when the instance starts:

- a numeric literal, optionally preceded by `-`, `true`, `false`, a string literal or a char literal;
- an enum case, a flags literal, or a constructor of a variant, `option` or `result` whose payload is a constant expression;
- a record literal, a list literal or a tuple whose values are constant expressions.

Anything else, such as an operator, a call, a local or another global, is E0409.

```wasl
global company: string = "Weasel & Co";
global default_line: invoices::line = invoices::line { label: "item", quantity: 1, unit_price: 0 };
global no_discount: option<invoices::discount> = option.none;
```

A global that is not `mut` always designates the same value, but that value can have `mut` fields or be a `list<mut T>`, whose contents can change. Its type is written with a path when it is a declared type (§ 7.8).

## 8.19 Error propagation `?`

`e?` unwraps a `result` or an `option`, and returns from the function in the other case:

```wasl
func quote(sku: string, quantity: u32) -> result<u64, cart_error> {
  local price: u64 = prices::of(sku)?;     // returns result.err(…) if `of` fails
  return result.ok(u64(quantity) * price);
}
```

- The operand is typed without an expected type (§ 8.2). Its type must be a `result` or an `option` (E0414).
- **On `result<T, E>`.** The function must return a `result<U, E>`, with any `U` and **the same** `E`, or no error payload on both sides (E0415). `e?` has type `T`. If `e` is `err(x)`, the function returns `result.err(x)` at once.
- **On `option<T>`.** The function must return an `option<U>`, with any `U` (E0415). `e?` has type `T`. If `e` is `none`, the function returns `option.none` at once.
- `?` never converts: a `result` cannot propagate into an `option` or the reverse, and an error type is never converted into another one (E0415). To change the error, write a `match`.
- On `result<_, E>`, `e?` has no value, like a call to a function without a return value (§ 8.6): it can only be used as a statement, `c.add(x)?;` (§ 9.6).
- A `return` through `?` releases the handles of the function like any `return` (§ 9.9).
- `?` is not a constant expression: it cannot appear in the initial value of a global (E0409).

`?` is the usual `match`, written in one character, and compiles to the same instructions (§ 12.11):

```wasl
local price: u64 = prices::of(sku)?;
// is
local price: u64;
match (prices::of(sku)) {
  case ok(p) { price = p; }
  case err(x) { return result.err(x); }
}
```

## 8.20 Resource calls

| Call | Meaning | Type |
|---|---|---|
| `r(args)`, `i::r(args)` | the constructor of the resource `r` | `r` |
| `e.m(args)` | the method `m` of the resource of `e`, called on `e` | the return type of `m` |
| `r::s(args)`, `i::r::s(args)` | the static function `s` of the resource `r` | the return type of `s` |

```wasl
local c: carts::cart = carts::cart();          // constructor
c.add(150, 2);                                  // method
local d: carts::cart = carts::cart::single(150);   // static function
```

- `r` is named like a type (§ 7.8): its name in its interface, the interface and its name elsewhere. A resource without constructor cannot be called (E0308).
- In `e.m(args)`, `e` is any expression whose type is a resource `r` or `borrow<r>`, and `m` a method of `r` (E0308). `e` is evaluated before the arguments. The method borrows `e` (§ 9.9).
- In `r::s(args)`, `s` is a static function of `r` (E0308). In a path of three names, `i::x::y(…)`, `x` is a variant (a constructor of § 8.14) or a resource (a static function), never both: types have one namespace (§ 7.7).
- Arguments and return values follow the rules of calls (§ 8.6): the number of arguments, their types, and a method without a return value used only as a statement.
- The resources of an imported interface are called the same way, with the names of their WIT converted to WaSL (§ 13.2): `out.blocking_write_and_flush(bytes)` calls the method `blocking-write-and-flush` of `wasi:io/streams`.

## 8.21 Async calls, streams, futures and `task.return`

Async calls, the built-in functions of streams and futures, and `task.return` are available.

### Async calls

- A call to an async function, an async method or an async static function, of the package or of an imported interface, and a call to an async built-in function (below), is an **async call**. It is only allowed in the body of an async function (E0416), wherever a call is allowed: as a statement, in an expression, in the arguments of another call.
- An async call has the type of the result of its function, like any call (§ 8.6), and waits for it (§ 2.8). `?` applies to it like to any other value: `client::send(req)?`.
- A function of an imported interface is async when its WIT says `async func`. An exported async function of the package is written `async func` in the WIT (§ 13.1).
- A foreign export block implements each function with the color of its WIT: `async func` for an `async func`, `func` for a `func` (E0218).

### Built-in functions of streams and futures

| Function | Async | Type | Result |
|---|---|---|---|
| `stream.new()` | no | `() -> tuple<stream<T>, stream_writer<T>>` | a new stream: its readable end, then its writable end, in the order of the Component Model |
| `stream.write(tx, xs)` | yes | `(stream_writer<T>, list<T>) -> u32` | writes the elements of `xs` in order, and waits until the reader has taken all of them; returns how many it wrote: `list.len(xs)`, or fewer if the readable end was released, after which every write returns `0` |
| `stream.read(rx, n)` | yes | `(stream<T>, u32) -> option<list<T>>` | waits for elements and returns `option.some` of 1 to `n` of them, in order; returns `option.none` once the writer has closed the stream and every element has been read, and on every read after that |
| `stream.close(tx)` | no | `(stream_writer<T>)` | closes the stream: after the last element, the reader gets `option.none` |
| `future.new()` | no | `() -> tuple<future<T>, future_writer<T>>` | a new future: its readable end, then its writable end |
| `future.write(tx, v)` | yes | `(future_writer<T>, T) -> bool` | gives `v` to the reader and waits until it has taken it; returns `false` if the readable end was released, and then releases the handles of `v` |
| `future.read(rx)` | yes | `(future<T>) -> T` | waits for the value of the future and returns it |

- As in § 8.17, a `list<T>` parameter accepts a `list<mut T>`.
- For `stream.new()` and `future.new()`, `T` comes from the expected type, from either position of the tuple: `local (rx: stream<u8>, tx: stream_writer<u8>) = stream.new();`. Without an expected type, they are E0408. For the other functions, `T` is the element type of the end they receive.
- `stream.write` and `stream.read` **borrow** their end, which stays in its local. `stream.close`, `future.write` and `future.read` **move** it and release it (§ 9.9): a future is written once and read once, and a closed stream is not written again.
- A `stream_writer<T>` is also closed when it is released at the end of its block (§ 9.9). `stream.close` closes it earlier, which is often needed: the standard output of WASI completes its future only once its stream is closed, so `hello` closes the stream before it reads that future (§ 15.7). It is the only function that releases a handle explicitly, because closing a stream has a meaning of its own: the end of the data.
- `stream.read(rx, 0)` takes no element: it waits until the writer writes or closes the stream, then returns `option.some([])` or `option.none`, as a read of length 0 does in the Component Model.
- `stream.write` writes everything: a write in the Component Model is a rendezvous that copies what the reader takes, sometimes less than what was offered, and `stream.write` repeats it until every element is taken. A read stays a single read, since reading by pieces is its purpose.
- `future.read` returns `T`, not an `option`: a future of WASI already holds a `result`, and a future whose writable end is released without a value has already trapped in the writer (§ 10.7).
- Their value is a temporary when it holds a handle (§ 7.12): `stream.new()` and `future.new()` are destructured at once, and the value of `future.read(rx)` is used by `match`, `?`, `return` or a destructuring, or discarded by a call statement (§ 9.6).

### `task.return`

```wasl
async func handle(request: types::request) -> result<types::response, types::error_code> {
  …                                            // prepare the response: `return result.err(…);` and `?` are allowed
  task.return(result.ok(response));          // the caller has the response now
  …                                            // write its body: only `return;` is allowed
}
```

`task.return(e);` is the `task.return` of the Component Model: it delivers `e`, the result of the task, to its caller, and the function goes on running. It exists for the results that the caller needs before the function can go on. A stream has no buffer (§ 10.12): a function that returns a `stream<u8>` must deliver it before it writes to it, or it would wait for ever for a reader that does not have the stream yet.

- `task.return(e);` is only allowed in an async function with a return type, and `e` has that type (E0417, § 8.1).
- It is a statement of the **top level** of the body, not inside an `if`, a `while` or a `match`, and a body contains at most one (E0417). So the result is delivered exactly once on every path, as the Component Model requires, and the check is syntactic.
- **Before it**, the body is an ordinary body: `return e;` and `?` deliver their value and end the function, for instance with an error before any response.
- **After it**, the result is delivered: `return e;` and `?` are errors (E0418), at any depth of the statements that follow, and `return;` ends the function, as does reaching the end of its body. For § 8.8, `task.return(e);` always returns.
- A handle that `e` holds is moved (§ 9.9).
- **Borrows end with it.** The Component Model ends the borrows of a task when the task delivers its result, and traps at `task.return` if the task still holds one. So the handles of the `borrow` parameters are given back at `task.return`, and a `borrow` parameter, `self` included, cannot be used after it (E0418): `local n: u32 = blob.size();` before `task.return`, not after.
- **A function that contains `task.return` is an entry point**: only the host and other components call it. A function reachable from the exports of a world cannot call a function that contains `task.return` compiled into the same component (E0419); it can call one of an interface that the world imports, which runs in another component. Inside a component, a call is a plain `call`, which returns at the end of the callee: the caller would wait for the whole function, and could not read a stream that the callee writes after delivering it. The check is a flag per function, like E0221. Concurrency inside a component will lift this rule (§ 16).
