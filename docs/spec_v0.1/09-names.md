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

Otherwise it is an error (E0300). Names of interfaces, worlds and functions are not values: `x = math;` is an unknown name.

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

- A local can have the name of an interface, a world or a function: they are never confused, since those names are only used in paths, `export` items and calls.
- A local declared without an initial value is **zero**: `0` for integers, `0.0` (positive zero) for floats, `false` for `bool`. This holds every time the declaration runs: a local declared inside a loop starts again from zero on every iteration.

## 9.5 Assignment

`x = e;` is valid if `x` is:

- a parameter or a local visible at that point;
- or a global declared `mut`.

Assigning an unknown name is an error (E0300); assigning a global that is not `mut` is an error (E0500). The value `e` has the type of `x` (§ 8.1).

## 9.6 Calls used as statements

A call followed by `;` is a statement. If the function returns a value, the value is discarded. This is the only expression that can be used as a statement (§ 5.3).

```wasl
reset();          // a function without a return value
incr();           // a function that returns a u32: the value is ignored
```

## 9.7 `break` and `continue`

- `break` and `continue` are only allowed inside a `while` (E0501).
- They act on the **innermost** enclosing `while`: `break` exits it, `continue` jumps to its next iteration (the condition is evaluated again).
- A `break` inside an `if` inside a loop exits the loop, not the `if`.
