# 9. Names and scopes

## 9.1 Functions

- Inside an interface, a function of the **same interface** is called by its name: `fib(n)`.
- A function of **another interface** of the package is called through a path: `math::fib(n)`. The path names an interface of the package; the current interface can also be named this way.
- Calling an unknown function is an error (E0301). If the name exists in another interface, the error message suggests the path.
- In a path `i::f`, `i` must be an interface: an unknown name, or the name of a world or a global, is an error (E0300).
- Functions are not values: a function name only appears in a call. It cannot be stored, passed or returned.
- Recursion is allowed, including mutual recursion and recursion across interfaces.

## 9.2 Globals

A global is designated by its name, from any function of any interface of the package: `calls = calls + 1;`. Globals are not qualified by a path.

## 9.3 Resolution of a name in an expression

A name used as a value is looked up:

1. among the parameters and locals visible at that point, from the innermost scope outwards;
2. then among the globals of the package.

Otherwise it is an error (E0300). Names of interfaces, worlds, functions and types are not values: `x = math;` is an unknown name. Enum cases and constructors are always written with a path (§ 9.8): `return paid;` is an unknown name, write `status::paid`.

## 9.4 Parameters and locals

- Parameters are **locals**, as in Wasm: they are visible in the whole body of the function and they can be **assigned**.
- A `local` is visible from the end of its declaration to the end of the block that contains it. Its initial value cannot use it: in `local q: u32 = q;`, `q` is unknown (E0300) unless it designates a global, which is itself forbidden (next rule).
- A parameter or a local **cannot have the name of a global** (E0303).
- A parameter or a local cannot have the name of another parameter or local that is visible at that point (E0304). There is no shadowing. Two sibling blocks (neither inside the other) can each declare a local with the same name:

```wasl
if (b) {
  local t: u32 = 1;
} else {
  local t: u32 = 2;     // valid: the first `t` is not visible here
}
```

- A local can have the name of an interface, a world, a function or a type: they are never confused, since those names are only used in paths, types, `export` items and calls.
- A binding of a `match` case (`case some(x)`) is a local of the block of that case (§ 8.16), with the same rules.
- A local declared without an initial value starts at the **default value of its Wasm type**: the value Wasm gives a local before its first `local.set`. Only scalars and options have one:

| Type | Wasm type (§ 12.5) | Default value |
|---|---|---|
| integers, floats, `bool` | `i32`, `i64`, `f32`, `f64` | `0`, `0.0` (positive zero), `false` |
| `char` | `i32` | `'\u{0}'` |
| an enum | `i32`, the index of the case | its first case |
| a flags type | `i32` | the empty set |
| `option<T>` | a nullable reference | `option::none` |

- A string, a list, a tuple, a record, a variant or a result is a non-nullable reference in Wasm, which has no default value: a local of one of these types must have an initial value (E0410). The language still has no `null` (§ 1.3): the null reference of an option is its `none`, a value of the option type.
- The default value is set every time the declaration runs: a local declared inside a loop starts from it again on every iteration. The default value of an enum depends on the order of its cases, as its representation does.

```wasl
local i: u32;               // 0
local s: status;            // status::draft, the first case
local d: option<discount>;  // option::none
local t: string;            // E0410: a string needs an initial value
```

## 9.5 Assignment

The target of an assignment `p = e;` (§ 5.3) is:

- a parameter or a local visible at that point;
- a global declared `mut`;
- `x.f`, where `f` is a `mut` field of the record `x`;
- `x[i]`, where `x` has type `list<mut T>`.

Assigning an unknown name is an error (E0300); assigning a global that is not `mut` is an error (E0500). Assigning a field that is not `mut`, an element of a `list<T>`, an element of a tuple or a flag of a flags value is E0505. To change flags, assign the whole value: `d = d | delivery { urgent };`. Assigning a field of a record stored in an immutable global is allowed when the field is `mut`: the global designates the record, not its contents. The value `e` has the type of the target (§ 8.1).

## 9.6 Calls used as statements

A call followed by `;` is a statement only if its function returns no value. Calling a function that returns a value as a statement is an error (E0413): the value would be lost without a trace, and with it the error of a `result`. To discard it on purpose, write `_ =` before the call.

```wasl
reset();          // a function without a return value
_ = incr();       // a function that returns a u32: the value is discarded
save(inv);        // E0413: `save` returns a result<_, save_error>, which would be lost
```

This is what Wasm itself requires: a value left on the stack must be removed with `drop`, and `_ = f();` is that `drop`, written in the source (§ 12.8).

- Only a call can follow `_ =` (§ 5.3): `_ = x;` is E0100.
- `_ = f();` on a function without a return value is E0400, as any other use of its call as a value (§ 8.6).
- The built-in functions all return a value: `string::len(s);` is E0413.

## 9.7 `break` and `continue`

- `break` and `continue` are only allowed inside a `while` (E0501).
- They act on the **innermost** enclosing `while`: `break` exits it, `continue` jumps to its next iteration (the condition is evaluated again).
- A `break` inside an `if` or a `match` inside a loop exits the loop, not the `if` or the `match`.

## 9.8 Paths

A path `a::b` or `a::b::c`, as an expression or before a call, is resolved by the kind of its first name:

| First name | Meaning | Examples |
|---|---|---|
| `string`, `list`, `char`, an integer type | a built-in function (§ 8.17) | `string::len(s)`, `list::filled(3, 0)`, `char::from_u32(n)`, `u32::wrapping_add(h, c)` |
| `option`, `result` | a constructor of the built-in type | `option::none`, `result::ok(sum)` |
| a type of the current interface | a case of that enum or variant | `status::paid`, `discount::fixed(500)` |
| an interface of the package, followed by one name | a function of that interface (§ 9.1) | `pricing::apply(sum, d)` |
| an interface of the package, followed by two names | a case of a type of that interface | `invoices::status::paid` |

An unknown first name is E0300 (§ 9.1); a path that names a type that is not an enum or a variant, or an unknown case, is E0307; a case written with the wrong payload is E0407 (§ 8.14). Types are named with paths of the same form (§ 7.8).
