# 8. Typing

## 8.1 General rule

Every expression has a single type, determined at compile time.

- Both operands of a binary operator have **exactly** the same type.
- A call argument has exactly the type of the corresponding parameter.
- An assigned value has exactly the type of its target.
- The value of `return` has exactly the return type of the function.
- The initial value of a `local` or a `global` has exactly its declared type.

Any mismatch is an error (E0400), e.g. `local y: u64 = n;` with `n: u32` (write `u64(n)`).

## 8.2 Literals

A literal has no type of its own: it takes the **type expected** by its context.

| Context | Expected type |
|---|---|
| `local x: T = <lit>;`, `global x: T = <lit>;` | `T` |
| `x = <lit>;` | the type of `x` |
| argument of a call | the type of the parameter |
| `return <lit>;` | the return type of the function |
| operand of a binary operator | see below |
| operand of unary `-`, expression in parentheses | the type expected for the enclosing expression |
| argument of a conversion `T(<lit>)` | none |
| condition of `if` / `while` | none (it must be a `bool`, § 8.9) |

**Literal-only expressions.** An expression is *literal-only* if it is an integer or float literal, or a parenthesized literal-only expression, or unary `-` applied to a literal-only expression, or an arithmetic, bitwise or shift operator (`+ - * / % & | ^ << >>`) applied to two literal-only expressions. `true` and `false` are not literal-only: they always have type `bool`.

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

## 8.3 Operators

| Operators | Operand type | Result type |
|---|---|---|
| `+` `-` `*` `/` | numeric, same type | same type |
| `%` | integer, same type | same type |
| `&` `\|` `^` | integer, same type | same type |
| `<<` `>>` | integer, same type | same type |
| `==` `!=` | any type, same type | `bool` |
| `<` `<=` `>` `>=` | numeric, same type | `bool` |
| `&&` `\|\|` | `bool` | `bool` |
| unary `-` | signed integer or float | same type |
| `!` | `bool` | `bool` |

Applying an operator to a type outside its column is an error (E0403):

- unary `-` on an unsigned integer or a `bool` (`-n` with `n: u32`);
- `%` on floats (Wasm has no float remainder instruction);
- `&`, `|`, `^`, `<<`, `>>` on floats or `bool`;
- `+`, `-`, `*`, `/`, `<`, `<=`, `>`, `>=` on `bool`;
- `&&`, `||`, `!` on anything other than `bool` (`if (n && b)` with `n: u32`).

The shift amount of `<<` and `>>` has the same type as the shifted value: `x << 3` with `x: u8` shifts by the `u8` literal `3`.

## 8.4 Precedence and associativity

From highest to lowest:

| Level | Operators | Associativity |
|---|---|---|
| 1 | call, conversion, `( … )` | — |
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

Bitwise operators bind tighter than comparisons, unlike in C: `a & b == c` reads `(a & b) == c`.

## 8.5 Conversions

A conversion is written as a call to the name of the target type: `u64(n)`, `f32(x)`, `s8(y)`.

- The argument must not be literal-only (E0402): there is no need to convert a literal, write a value of the target type directly. This is checked first: `u8(5)` and `bool(1)` are E0402.
- The argument must have a numeric type and the target type must be numeric. Converting from or to `bool` is an error (E0404): `u32(true)`, `bool(n)` with `n: u32`.
- A conversion to the same type is allowed and has no effect.
- The value produced by each conversion is specified in § 10.5.

## 8.6 Calls

- A call `f(args)` or `i::f(args)` designates a function as described in § 9.1.
- The number of arguments must be the number of parameters (E0302).
- Each argument has the type of its parameter (§ 8.1); literals take that type.
- A call to a function without a return value is not a value: using it in an expression is an error (E0400), e.g. `local x: u32 = reset();`. It can only be used as a statement.

## 8.7 Return

- In a function with return type `T`, every `return` carries an expression of type `T`. A `return;` without value is an error (E0502).
- In a function without a return type, `return;` is allowed and `return <expr>;` is an error (E0503). Reaching the end of the body is an implicit `return;`.

## 8.8 Every path returns

A function with a return type must end with a `return` on **every** execution path (E0504). The check is syntactic:

- a block *always returns* if one of its statements always returns;
- a `return` always returns;
- an `if` always returns if it has an `else` branch and every branch (including the branches of `else if`s) always returns;
- a `while` never always returns, whatever its condition (`while (true) { return 1; }` does not count).

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
