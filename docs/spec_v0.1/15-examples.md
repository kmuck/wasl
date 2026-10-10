# 15. Examples

## 15.1 A complete project

This project is in [`examples/calc`](../../examples/calc) at the root of the repository.

`wasl.toml`:

```toml
[package]
namespace = "example"
name = "calc"
version = "0.1.0"
```

`src/calc.wasl`:

```wasl
world calculator {
  export math;
}

global pi: f64 = 3.14159;
global calls: mut u32 = 0;

interface math {

  func fib(n: u32) -> u64 {
    calls = calls + 1;
    local a: u64 = 0;
    local b: u64 = 1;
    local i: u32;
    while (i < n) {
      local next: u64 = a + b;
      a = b;
      b = next;
      i = i + 1;
    }
    return a;
  }

  func fib_rec(n: u32) -> u64 {
    calls = calls + 1;
    if (n <= 1) {
      return u64(n);
    }
    return fib_rec(n - 1) + fib_rec(n - 2);
  }

  func circle_area(r: f64) -> f64 {
    calls = calls + 1;
    return pi * r * r;
  }

  func clamp(x: s32, lo: s32, hi: s32) -> s32 {
    calls = calls + 1;
    return util::clamp(x, lo, hi);
  }

  func get_calls() -> u32 {
    return calls;
  }

  func reset() {
    calls = 0;
  }

}

interface util {

  func clamp(x: s32, lo: s32, hi: s32) -> s32 {
    if (x < lo) {
      return lo;
    } else if (x > hi) {
      return hi;
    }
    return x;
  }

}
```

What this program shows:

- `math` is exported by the world `calculator`: its six functions are the interface of the component.
- `util` is exported by no world: it is internal, and `math::clamp` calls it with a path (`util::clamp`).
- `pi` is an immutable global and `calls` a mutable one, shared by all the functions of the package.
- `i` and the locals without initial value start at zero; `next` is declared inside the loop.
- `fib_rec` is recursive and converts `n` explicitly with `u64(n)`.

WIT of the package (`weasel wit`):

```wit
package example:calc@0.1.0;

interface math {
  fib: func(n: u32) -> u64;
  fib-rec: func(n: u32) -> u64;
  circle-area: func(r: f64) -> f64;
  clamp: func(x: s32, lo: s32, hi: s32) -> s32;
  get-calls: func() -> u32;
  reset: func();
}

world calculator {
  export math;
}
```

Session:

```console
$ weasel build calculator
Built `build/calculator.wasm`
$ weasel call math::fib 10
55
$ weasel call fib_rec 20
6765
$ weasel call math::circle_area 2
12.56636
$ weasel call math::clamp -50 0 10
0
$ weasel call math::reset
$ weasel call math::fib
error: `math::fib` expects 1 argument(s) (n: u32), 0 given
```

## 15.2 Several worlds

One package, three components sharing the same code:

```wasl
// src/worlds.wasl
world full {
  export math;
  export stats;
}

world math_only {
  export math;
}

world nothing {
}
```

```wasl
// src/math.wasl
interface math {
  func square(x: s64) -> s64 {
    return x * x;
  }
}

// src/stats.wasl
interface stats {
  func mean(a: f64, b: f64) -> f64 {
    return (a + b) / 2.0;
  }
}
```

- `weasel build full`, `weasel build math_only` and `weasel build nothing` produce three components.
- `math::square` is the same code in `full` and `math_only` (§ 2.4.1); `math_only` does not contain `stats`.
- `weasel call math::square 7` prints `49` without asking for a world.

## 15.3 Invalid programs

Each line is a statement or a declaration placed in a valid context, with the error it produces.

| Code | Error |
|---|---|
| `local x = 5;` | E0100: the type of a local is required |
| `local x: u8 = 256;` | E0401: literal out of range |
| `local x: s8 = 128;` | E0401: `s8` stops at 127 |
| `local x: f64 = 1;` | E0400: integer literal for a float type, write `1.0` |
| `local y: u64 = n;` (`n: u32`) | E0400: mismatched types, write `u64(n)` |
| `local z: u32 = -1;` | E0403: `-` on an unsigned type |
| `if (n) { … }` (`n: u32`) | E0405: non-boolean condition, write `n != 0` |
| `if (n > 0) return n;` | E0100: braces are required |
| `if n > 0 { … }` | E0100: parentheses are required |
| `if (a < b < c) { … }` | E0100: comparisons are not associative |
| `if (1 < 2) { … }` | E0402: no type for the literals |
| `x + 1;` | E0100: only calls and assignments are statements |
| `i += 1;` | E0100: no compound assignment |
| `pi = 3.0;` (`global pi: f64`) | E0500: `pi` is not `mut` |
| `local pi: f64 = 1.0;` | E0303: name of a global |
| `local n: u32 = 1;` (parameter `n`) | E0304: already declared |
| `return -x;` (`x: u32`) | E0403: `-` on an unsigned type |
| `local r: f64 = a % b;` (`a, b: f64`) | E0403: `%` on floats |
| `local w: u8 = u8(5);` | E0402: there is no need to convert a literal |
| `local c: u32 = u32(b);` (`b: bool`) | E0404: `bool` is not numeric |
| `local v: u32 = reset();` (`reset` returns nothing) | E0400: no value |
| `break;` outside a loop | E0501 |
| `return;` in `func f() -> u32` | E0502 |
| `return 1;` in `func f()` | E0503 |
| `func f(x: u32) -> u32 { if (x > 0) { return x; } }` | E0504: no value returned when `x == 0` |
| `global g: u32 = 1 + 1;` | E0100: the initial value of a global is a literal |
| `interface i { global g: u32 = 0; }` | E0100: globals are top-level declarations |
| `world w { export nope; }` | E0203: unknown interface |
| `world w { export wasi:cli/run@0.3.0 { … } }` | E0001: `@` is not a valid character in v0.1; exporting a foreign interface is not available (§ 2.4.2) |
| `world w { export wasi:cli/run { … } }` | E0100: only the name of an interface of the package can follow `export` in v0.1 |
| `local record: u32 = 1;` | E0005: reserved word |
| `local Total: u32 = 1;` | E0004: uppercase letter |
| `local q: u32 = 007;` | E0002: leading zero |
| `local q: f64 = 1.;` | E0003: a digit is required after the point |
