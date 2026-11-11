# 15. Examples

## 15.1 Numbers and functions

This project is in [`examples/calc`](../../examples/calc) at the root of the repository. It is the target program of v0.1.

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
- `i` and the locals without initial value start at zero (§ 9.4); `next` is declared inside the loop.
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
$ weasel call calculator math::fib 10
55
$ weasel call calculator math::fib_rec 20
6765
$ weasel call calculator math::circle_area 2
12.56636
$ weasel call calculator math::clamp -50 0 10
0
$ weasel call calculator math::reset
$ weasel call calculator math::fib
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
- `weasel call full math::square 7` and `weasel call math_only math::square 7` both print `49`: the same code runs in both components.

## 15.3 Data on the WasmGC heap

This project is in [`examples/invoice`](../../examples/invoice). It is the target program of v0.2: it was written before the v0.2 specification, as the program v0.2 must compile, and every rule of the heap types was checked against it.

`wasl.toml`:

```toml
[package]
namespace = "example"
name = "invoice"
version = "0.1.0"
```

`src/invoice.wasl`:

```wasl
// Target program of WaSL v0.2: data on the WasmGC heap (docs/spec_v0.2).
// It is written before the compiler supports it, and the v0.1 compiler rejects it.

world billing {
  export invoices;
}

global company: string = "Weasel & Co";

interface invoices {

  enum status {
    draft,
    sent,
    paid,
  }

  flags delivery {
    email,
    paper,
    urgent,
  }

  record line {
    label: string,
    quantity: u32,
    unit_price: u64,
  }

  variant discount {
    percent(u8),
    fixed(u64),
  }

  record invoice {
    number: u32,
    customer: string,
    lines: list<line>,
    discount: option<discount>,
    delivery: delivery,
    status: status,
  }

  enum invoice_error {
    empty_customer,
    no_lines,
    zero_quantity,
    discount_too_large,
  }

  func sample() -> invoice {
    return invoice {
      number: 42,
      customer: "ACME",
      lines: [
        line { label: "pen", quantity: 3, unit_price: 150 },
        line { label: "notebook", quantity: 2, unit_price: 425 },
      ],
      discount: option.some(discount::percent(10)),
      delivery: delivery { email, urgent },
      status: status::draft,
    };
  }

  // Total in cents, after the discount.
  func total(inv: invoice) -> result<u64, invoice_error> {
    if (string.len(inv.customer) == 0) {
      return result.err(invoice_error::empty_customer);
    }
    if (list.len(inv.lines) == 0) {
      return result.err(invoice_error::no_lines);
    }
    local sum: u64;
    local i: u32;
    while (i < list.len(inv.lines)) {
      local l: line = inv.lines[i];
      if (l.quantity == 0) {
        return result.err(invoice_error::zero_quantity);
      }
      sum = sum + u64(l.quantity) * l.unit_price;
      i = i + 1;
    }
    match (inv.discount) {
      case some(d) {
        return pricing::apply(sum, d);
      }
      case none {
        return result.ok(sum);
      }
    }
  }

  // An invoice crosses the boundary of the component, so it cannot change
  // (§ 7.11): a paid invoice is a new record.
  func mark_paid(inv: invoice) -> result<invoice, invoice_error> {
    match (total(inv)) {
      case ok {
        return result.ok(invoice {
          number: inv.number,
          customer: inv.customer,
          lines: inv.lines,
          discount: inv.discount,
          delivery: inv.delivery,
          status: status::paid,
        });
      }
      case err(e) {
        return result.err(e);
      }
    }
  }

  func summary(inv: invoice) -> string {
    return company + ": invoice " + text::from_u32(inv.number) + " for " + inv.customer
      + " (" + status_name(inv.status) + ")";
  }

  func status_name(s: status) -> string {
    match (s) {
      case draft {
        return "draft";
      }
      case sent {
        return "sent";
      }
      case paid {
        return "paid";
      }
    }
  }

  func needs_printing(inv: invoice) -> bool {
    return inv.delivery.paper;
  }

  // First letter of the customer's name.
  func initial(inv: invoice) -> option<char> {
    local chars: list<char> = string.chars(inv.customer);
    if (list.len(chars) == 0) {
      return option.none;
    }
    return option.some(chars[0]);
  }

  // Number of invoices, and sum of the totals that are valid.
  func stats(all: list<invoice>) -> tuple<u32, u64> {
    local sum: u64;
    local i: u32;
    while (i < list.len(all)) {
      match (total(all[i])) {
        case ok(t) {
          sum = sum + t;
        }
        case err {
        }
      }
      i = i + 1;
    }
    return tuple(list.len(all), sum);
  }

  func find(all: list<invoice>, customer: string) -> option<invoice> {
    local i: u32;
    while (i < list.len(all)) {
      if (all[i].customer == customer) {
        return option.some(all[i]);
      }
      i = i + 1;
    }
    return option.none;
  }

  func count_unpaid(all: list<invoice>) -> u32 {
    local count: u32;
    local i: u32;
    while (i < list.len(all)) {
      if (all[i].status != status::paid) {
        count = count + 1;
      }
      i = i + 1;
    }
    return count;
  }

}

// Internal: exported by no world, so it appears in no WIT.
interface pricing {

  func apply(sum: u64, d: invoices::discount) -> result<u64, invoices::invoice_error> {
    match (d) {
      case percent(p) {
        if (p > 100) {
          return result.err(invoices::invoice_error::discount_too_large);
        }
        return result.ok(sum - sum * u64(p) / 100);
      }
      case fixed(amount) {
        if (amount > sum) {
          return result.err(invoices::invoice_error::discount_too_large);
        }
        return result.ok(sum - amount);
      }
    }
  }

}

// Internal: text formatting, written in WaSL on top of `string` and `list`.
interface text {

  // Decimal digits of `n`, built byte by byte in a mutable list.
  func from_u32(n: u32) -> string {
    local count: u32 = 1;
    local rest: u32 = n / 10;
    while (rest != 0) {
      count = count + 1;
      rest = rest / 10;
    }
    local bytes: list<mut u8> = list.filled(count, 0);
    rest = n;
    while (count > 0) {
      count = count - 1;
      bytes[count] = u8(rest % 10) + 48;
      rest = rest / 10;
    }
    match (string.from_bytes(bytes)) {
      case some(s) {
        return s;
      }
      case none {
        return "";
      }
    }
  }

}
```

What this program shows:

- `invoices` declares six types next to its functions, as a WIT interface does; `pricing` names them with a path (`invoices::discount`).
- `line` and `invoice` are records without `mut` field: they appear in the WIT, which has no mutability (§ 7.11).
- `discount` is a variant whose cases both carry a payload; `invoice.discount` is optional.
- `delivery` is a flags type: `sample` builds the set `delivery { email, urgent }`, and `needs_printing` tests one flag with `inv.delivery.paper`.
- `initial` decodes the customer's name with `string.chars` and returns an `option<char>`; `stats` returns two values in a `tuple<u32, u64>`.
- `total` returns a `result`; `match` is the only way to look inside an option, a result or a variant.
- `company` is a global string, initialized with a constant (§ 8.18).
- `find` compares strings with `==` and returns an `option`; `count_unpaid` compares enums with `!=`.
- `text::from_u32` builds a string from a `list<mut u8>` with the built-in functions only: number formatting belongs to a library, not to the compiler.
- `mark_paid` returns a new invoice with the status `paid`: its argument cannot change, so it behaves the same whether it is called from outside or from inside the component (§ 10.10).

WIT of the package (`weasel wit`):

```wit
package example:invoice@0.1.0;

interface invoices {
  enum status {
    draft,
    sent,
    paid,
  }

  flags delivery {
    email,
    paper,
    urgent,
  }

  record line {
    label: string,
    quantity: u32,
    unit-price: u64,
  }

  variant discount {
    percent(u8),
    fixed(u64),
  }

  record invoice {
    number: u32,
    customer: string,
    lines: list<line>,
    discount: option<discount>,
    delivery: delivery,
    status: status,
  }

  enum invoice-error {
    empty-customer,
    no-lines,
    zero-quantity,
    discount-too-large,
  }

  sample: func() -> invoice;
  total: func(inv: invoice) -> result<u64, invoice-error>;
  mark-paid: func(inv: invoice) -> result<invoice, invoice-error>;
  summary: func(inv: invoice) -> string;
  status-name: func(s: status) -> string;
  needs-printing: func(inv: invoice) -> bool;
  initial: func(inv: invoice) -> option<char>;
  stats: func(all: list<invoice>) -> tuple<u32, u64>;
  find: func(all: list<invoice>, customer: string) -> option<invoice>;
  count-unpaid: func(all: list<invoice>) -> u32;
}

world billing {
  export invoices;
}
```

`pricing` and `text` are internal (§ 2.2) and use no type of their own in an exported signature, so they do not appear.

Session (the invoice `$INV` is the result of `sample`):

```console
$ weasel build billing
Built `build/billing.wasm`
$ weasel call billing invoices::sample
{number: 42, customer: "ACME", lines: [{label: "pen", quantity: 3, unit-price: 150}, {label: "notebook", quantity: 2, unit-price: 425}], discount: some(percent(10)), delivery: {email, urgent}, status: draft}
$ INV='{number: 42, customer: "ACME", lines: [{label: "pen", quantity: 3, unit-price: 150}, {label: "notebook", quantity: 2, unit-price: 425}], discount: some(percent(10)), delivery: {email, urgent}, status: draft}'
$ weasel call billing invoices::total "$INV"
ok(1170)
$ weasel call billing invoices::summary "$INV"
"Weasel & Co: invoice 42 for ACME (draft)"
$ weasel call billing invoices::mark_paid "$INV"
ok({number: 42, customer: "ACME", lines: [{label: "pen", quantity: 3, unit-price: 150}, {label: "notebook", quantity: 2, unit-price: 425}], discount: some(percent(10)), delivery: {email, urgent}, status: paid})
$ weasel call billing invoices::total '{number: 7, customer: "", lines: [], discount: none, delivery: {}, status: draft}'
err(empty-customer)
$ weasel call billing invoices::total '{number: 7, customer: "Zed", lines: [{label: "x", quantity: 1, unit-price: 100}], discount: some(fixed(500)), delivery: {paper}, status: sent}'
err(discount-too-large)
$ weasel call billing invoices::count_unpaid "[$INV]"
1
$ weasel call billing invoices::initial "$INV"
some('A')
$ weasel call billing invoices::needs_printing "$INV"
false
$ weasel call billing invoices::stats "[$INV, $INV]"
(2, 2340)
$ weasel call billing invoices::find "[$INV]" '"Zed"'
none
```

## 15.4 Invalid programs

Each line is a statement or a declaration placed in a valid context, with the error it produces.

### With numbers

| Code | Error |
|---|---|
| `local x;` | E0100: a local needs a type or an initial value |
| `local x: u8 = 256;` | E0401: literal out of range |
| `local x: s8 = 128;` | E0401: `s8` stops at 127 |
| `local x: f64 = 1;` | E0400: integer literal for a float type, write `1.0` |
| `local y: u64 = n;` (`n: u32`) | E0400: mismatched types, write `u64(n)` |
| `local z: u32 = -1;` | E0403: `-` on an unsigned type |
| `if (n) { … }` (`n: u32`) | E0405: non-boolean condition, write `n != 0` |
| `if (a) if (b) f();` | E0100: a body without braces is a simple statement; write braces |
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
| `global g: u32 = 1 + 1;` | E0409: the initial value of a global is a constant expression |
| `interface i { global g: u32 = 0; }` | E0100: globals are top-level declarations |
| `world w { export nope; }` | E0203: unknown interface |
| `world w { export wasi:cli/run@0.3.0 { … } }` | E0001: `@` is not a valid character; the version of a dependency is written in `wasl.toml` (§ 3.5) |
| `world w { export wasi:cli/run { … } }` without dependencies | E0215: `wasi:cli` is not in `[dependencies]` (§ 3.5) |
| `local record: u32 = 1;` | E0005: reserved word |
| `local Total: u32 = 1;` | E0004: uppercase letter |
| `local q: u32 = 007;` | E0002: leading zero |
| `local q: f64 = 1.;` | E0003: a digit is required after the point |

### With the types of the invoice program

Each line is placed in a valid context, with the types of § 15.3.

| Code | Error |
|---|---|
| `local s: string = "abc;` | E0006: the literal is not closed |
| `local s: string = "\q";` | E0006: unknown escape |
| `record r { }` | E0100: a record has at least one field |
| `world w { record r { a: u32 } }` | E0100: types are declared in interfaces |
| `record line { … }` and `func line()` in one interface | E0206: one namespace for types and functions |
| `record r { a: u32, a: u64 }` | E0207: field declared twice |
| `record math { a: u32 }` with `interface math` | E0210: name of a top-level declaration |
| `record a { next: a }` | E0211: no value of `a` can be built |
| `func root() -> node` exported, `record node { children: list<node> }` | E0208: recursive types cannot cross the boundary |
| `record counter { n: mut u32 }` in `invoices` | E0214: the types of an exported interface appear in the WIT, which has no `mut` |
| `func sort(xs: list<mut u32>)` in `invoices` | E0214: use `list<u32>`, and `list.copy` inside the function |
| `local x: lines = …;` | E0305: unknown type |
| `local l: line = …;` in `pricing` | E0305: from another interface, write `invoices::line` |
| `inv.total` | E0306: `invoice` has no field `total` |
| `status::lost` | E0307: `status` has no case `lost` |
| `line { quantity: 1, label: "pen", unit_price: 5 }` | E0406: fields in declaration order |
| `line { label: "pen", quantity: 1 }` | E0406: missing field `unit_price` |
| `status::paid()` | E0407: `paid` has no payload |
| `discount::percent` | E0407: `percent` needs a payload |
| `case none(x) { }` | E0407: `none` has no payload |
| `local d: u8 = option.none;` | E0400: `option.none` is not a `u8` |
| `match (option.none) { … }` | E0408: no type for `option.none` |
| `local n: u32 = list.len([]);` | E0408: no element type for `[]` |
| `global s: string = "a" + "b";` | E0409: an operator is not a constant expression |
| `local s: string;` | E0410: a string needs an initial value |
| `local m: line;` | E0410: a record needs an initial value |
| `_ = fib(3);` | E0100: there is no `_ =`; write `fib(3);`, which discards the value |
| `match (n) { … }` with `n: u32` | E0411: use `if` on numbers |
| `inv.number = 7;` | E0505: `number` is not `mut` |
| `inv.status = status::paid;` | E0505: `status` is not `mut`; build a new `invoice` |
| `inv.lines[0] = l;` | E0505: `lines` is a `list<line>`, not a `list<mut line>` |
| `match (s) { case draft { } case sent { } }` | E0506: `paid` is not covered |
| `match (s) { case draft { } case sent { } case paid { } else { } }` | E0507: the `else` covers no case |
| `if (d == e) { }` with `d: list<mut u8>` | E0403: a `list<mut u8>` can change; compare its elements |
| `if (inv.customer < "M") { }` | E0403: strings have no order |
| `local c: u8 = inv.customer[0];` | E0403: strings cannot be indexed; use `string.bytes(inv.customer)[0]` |
| `local xs: list<mut u8> = string.bytes(s);` | E0400: a `list<u8>` is not a `list<mut u8>`; use `list.copy` |
| `return d;` with `d: list<mut u8>` in `func f() -> list<u8>` | E0400: a `list<mut u8>` is not a `list<u8>`; use `list.freeze(d)` |
| `local c: char = '';` | E0007: a char literal holds exactly one character |
| `local c: char = char(n);` | E0404: use `char.from_u32(n)`, which can fail |
| `local n: u64 = u64(c);` with `c: char` | E0404: a `char` only converts to `u32` |
| `local d: delivery = delivery { urgent, email };` | E0412: flags in declaration order |
| `inv.delivery.paper = true;` | E0505: write `d = d \| delivery { paper };` on a local |
| `local t: tuple<u32, u64> = tuple(1, 2, 3);` | E0400: three elements for a tuple of two |
| `stats(all).2` | E0306: the tuple has two elements |
| `return paid;` | E0300: write `status::paid` |
| `f().number = 1;` | E0100: an assignment target starts with a name |

### With imports, resources and `?`

Each line is written in `examples/checkout` (§ 15.5), unless it says otherwise.

| Code | Error |
|---|---|
| `import example:nope/carts;` | E0215: `example:nope` is not in `[dependencies]` |
| `import example:catalog/nope;` | E0215: `example:catalog` has no interface `nope` |
| `import example:catalog/carts;` twice in `checkout` | E0216: an interface is imported once |
| `interface carts { … }` in `checkout` | E0217: `carts` already names `example:catalog/carts` |
| `export wasi:cli/run { func run() -> u32 { … } }` in `hello` | E0218: `run` returns `result` in `wasi:cli/run` |
| `export wasi:cli/run { }` in `hello` | E0218: the function `run` is missing |
| `func keep(c: carts::cart) -> borrow<carts::cart>` | E0219: `borrow` is only for parameters |
| `return c;` with `c: borrow<carts::cart>` | E0219: a borrowed handle cannot be given away |
| `record basket { cart: carts::cart }` | E0220: a handle cannot be stored in a record |
| `local o: option<carts::cart> = option.none;` | E0220: write `local c: carts::cart = …;` and handle the `none` with `?` or `match` |
| `c.remove("pen")` | E0308: `cart` has no method `remove` |
| `c.items` | E0309: the state of a cart is private to `carts` |
| `local n: u32 = c.count()?;` | E0414: `count` returns a `u32`, not a `result` or an `option` |
| `local c: carts::cart = carts::cart::of("pen", 1)?;` in `func f() -> option<u64>` | E0415: the error of a `result` cannot propagate into an `option` |
| `local c: carts::cart = carts::cart::of("pen", 1)?;` in `func f() -> u64` | E0415: `f` must return a `result<…, carts::cart_error>` |
| `stdout::get_stdout()` in `checkout` | E0300: `stdout` is not an interface that this file can name; write `use wasi:cli/stdout;`, add `wasi:cli` to the dependencies, and import it in the world |
| in `hello`, `world quiet { export greeting; }` with a call to `stdout::get_stdout()` in `greeting::text` | E0221: `greeting::text` is reachable from `quiet`, which does not import `wasi:cli/stdout` |

## 15.5 Two components: `catalog` and `checkout`

The target program of version 0.3 is two projects. `examples/catalog` defines a resource, a shopping cart, and `examples/checkout` uses it from another component: it imports the interface of `catalog`, creates carts and calls their methods. The two components are built separately and composed when they run.

`examples/catalog/wasl.toml` declares `example:catalog@0.1.0`, without dependencies. `examples/catalog/src/catalog.wasl`:

```wasl
world catalog {
  export carts;
}

interface carts {

  enum cart_error {
    unknown_sku,
    zero_quantity,
  }

  record line {
    sku: string,
    quantity: u32,
  }

  // A cart lives in the instance of `catalog`; other components hold a handle to it.
  resource cart {
    // Private state: it never appears in the WIT, so `mut` is allowed (§ 7.12).
    items: mut u32,
    amount: mut u64,
    last: mut option<line>,

    constructor() {
      return cart { items: 0, amount: 0, last: option.none };
    }

    func add(sku: string, quantity: u32) -> result<_, cart_error> {
      if (quantity == 0) {
        return result.err(cart_error::zero_quantity);
      }
      local price: u64 = prices::of(sku)?;
      self.items = self.items + quantity;
      self.amount = self.amount + u64(quantity) * price;
      self.last = option.some(line { sku: sku, quantity: quantity });
      return result.ok;
    }

    func total() -> u64 {
      return self.amount;
    }

    func count() -> u32 {
      return self.items;
    }

    func last_line() -> option<line> {
      return self.last;
    }

    // A cart with a single line, built in one call.
    static func of(sku: string, quantity: u32) -> result<cart, cart_error> {
      local c: cart = cart();
      c.add(sku, quantity)?;
      return result.ok(c);
    }
  }

}

// Internal: exported by no world, so it appears in no WIT.
interface prices {

  // Unit price in cents.
  func of(sku: string) -> result<u64, carts::cart_error> {
    if (sku == "pen") {
      return result.ok(150);
    }
    if (sku == "notebook") {
      return result.ok(425);
    }
    return result.err(carts::cart_error::unknown_sku);
  }

}
```

Its WIT:

```wit
package example:catalog@0.1.0;

interface carts {
  enum cart-error {
    unknown-sku,
    zero-quantity,
  }
  record line {
    sku: string,
    quantity: u32,
  }
  resource cart {
    constructor();
    add: func(sku: string, quantity: u32) -> result<_, cart-error>;
    total: func() -> u64;
    count: func() -> u32;
    last-line: func() -> option<line>;
    of: static func(sku: string, quantity: u32) -> result<cart, cart-error>;
  }
}

world catalog {
  export carts;
}
```

`examples/checkout/wasl.toml`:

```toml
[package]
namespace = "example"
name = "checkout"
version = "0.1.0"

[dependencies]
"example:catalog" = { path = "../catalog" }
```

`examples/checkout/src/checkout.wasl`:

```wasl
use example:catalog/carts;

world checkout {
  import example:catalog/carts;
  export orders;
}

interface orders {

  record item {
    sku: string,
    quantity: u32,
  }

  record receipt {
    items: u32,
    total: u64,
  }

  // Each `cart` below is a handle to a cart that lives in `catalog`. It is given back
  // to `catalog` when the function returns, including the early returns of `?`.
  func checkout(items: list<item>) -> result<receipt, carts::cart_error> {
    local c: carts::cart = carts::cart();
    local i: u32;
    while (i < list.len(items)) {
      c.add(items[i].sku, items[i].quantity)?;
      i = i + 1;
    }
    return result.ok(receipt { items: c.count(), total: c.total() });
  }

  func quote(sku: string, quantity: u32) -> result<u64, carts::cart_error> {
    local c: carts::cart = carts::cart::of(sku, quantity)?;
    return result.ok(c.total());
  }

  // The cart is lent to `describe` for the duration of the call.
  func last_sku(items: list<item>) -> option<string> {
    local c: carts::cart = carts::cart();
    local i: u32;
    while (i < list.len(items)) {
      match (c.add(items[i].sku, items[i].quantity)) {
        case ok { }
        case err {
          return option.none;
        }
      }
      i = i + 1;
    }
    return describe(c);
  }

  func describe(c: borrow<carts::cart>) -> option<string> {
    local l: carts::line = c.last_line()?;
    return option.some(l.sku);
  }

}
```

Its WIT:

```wit
package example:checkout@0.1.0;

interface orders {
  use example:catalog/carts@0.1.0.{cart, cart-error};

  record item {
    sku: string,
    quantity: u32,
  }
  record receipt {
    items: u32,
    total: u64,
  }
  checkout: func(items: list<item>) -> result<receipt, cart-error>;
  quote: func(sku: string, quantity: u32) -> result<u64, cart-error>;
  last-sku: func(items: list<item>) -> option<string>;
  describe: func(c: borrow<cart>) -> option<string>;
}

world checkout {
  import example:catalog/carts@0.1.0;
  export orders;
}
```

```sh
cd examples/checkout
weasel build checkout       # → build/checkout.wasm, which imports example:catalog/carts@0.1.0
weasel call checkout orders::checkout '[{sku: "pen", quantity: 2}, {sku: "notebook", quantity: 1}]'
# → ok({items: 3, total: 725})
weasel call checkout orders::checkout '[{sku: "pen", quantity: 2}, {sku: "ink", quantity: 1}]'
# → err(unknown-sku)
weasel call checkout orders::quote '"pen"' 0                       # → err(zero-quantity)
weasel call checkout orders::last_sku '[{sku: "pen", quantity: 1}]'  # → some("pen")
weasel call checkout orders::last_sku '[]'                          # → none
```

What the program shows:

- **A resource of the package and its handles.** In `catalog`, a cart is a WasmGC struct, and `cart::of` calls the constructor and a method like any function (§ 12.10). In `checkout`, the same cart is an `i32` handle; every `c.add(…)` is a call to the other component (§ 13.10).
- **Automatic release.** `checkout` never releases a cart in its source: each one is given back to `catalog` at the end of the function, on the `?` that leaves early and on the `return` (§ 9.9). `describe` borrows the cart, so `last_sku` still owns it after the call.
- **`?` on `result` and `option`.** `prices::of(sku)?` and `c.add(…)?` propagate a `cart_error`; `c.last_line()?` propagates a `none` (§ 8.19).
- **Composition.** `weasel call` builds `catalog` from its path dependency and plugs it into `checkout` (§ 14.2). The same composition is written with the standard tool: `wac plug build/checkout.wasm --plug ../catalog/build/catalog.wasm -o build/shop.wasm`.

## 15.6 A WASI command: `hello`

The second target program of version 0.3 is a command-line program for WASI 0.2: its world exports `wasi:cli/run` and imports what it needs to write on its standard output. It runs with `wasmtime run`, like a program written in any other language. It was `examples/hello` in 0.3; from 0.4 on, `examples/hello` is the WASI 0.3 program of § 15.7, and this one stays valid.

`examples/hello/wasl.toml`:

```toml
[package]
namespace = "example"
name = "hello"
version = "0.1.0"

[dependencies]
"wasi:cli" = "0.2.6"
"wasi:io" = "0.2.6"
```

`examples/hello/src/hello.wasl`:

```wasl
use wasi:io/streams;
use wasi:cli/stdout;

world hello {
  import wasi:io/streams;
  import wasi:cli/stdout;

  export wasi:cli/run {
    func run() -> result {
      local out: streams::output_stream = stdout::get_stdout();
      match (out.blocking_write_and_flush(string.bytes(greeting::text("world")))) {
        case ok {
          return result.ok;
        }
        case err {
          return result.err;
        }
      }
      // `out` is given back to the host when `run` returns.
    }
  }
}

interface greeting {
  func text(name: string) -> string {
    return "Hello, " + name + "!\n";
  }
}
```

Its WIT, where `greeting`, internal, does not appear:

```wit
package example:hello@0.1.0;

world hello {
  import wasi:io/streams@0.2.6;
  import wasi:cli/stdout@0.2.6;
  export wasi:cli/run@0.2.6;
}
```

```sh
cd examples/hello
weasel build hello                 # fetches wasi:cli and wasi:io, writes wkg.lock
wasmtime run -W gc=y -W function-references=y build/hello.wasm
# Hello, world!
weasel run hello                   # the same with weasel (§ 14.2), from 0.4 on
```

What the program shows:

- **A foreign export.** `run` implements the function of `wasi:cli/run` in the world, with the signature of its WIT, `run: func() -> result;` (§ 2.4.2, § 13.12).
- **Imported functions and resources of WASI.** `stdout::get_stdout()` returns an `output-stream` of `wasi:io/streams`, a handle provided by the host; `out.blocking_write_and_flush(…)` is a method call on it, and the stream is released when `run` returns (§ 9.9). The error of the write, a `stream-error` that holds a handle of its own, is released by the `case err` that does not name it.
- **Registry dependencies.** `wasi:cli` and `wasi:io` are fetched from `ghcr.io/webassembly` with the crates of `wkg`, and pinned in `wkg.lock` (§ 3.5).

## 15.7 A WASI 0.3 command: `hello`

This is `examples/hello` for WASI 0.3, one of the two target programs of version 0.4. It replaces the WASI 0.2 program of § 15.6, and CI runs it with `weasel run hello`.

`examples/hello/wasl.toml`:

```toml
[package]
namespace = "example"
name = "hello"
version = "0.1.0"

[dependencies]
"wasi:cli" = "0.3.0"
```

`examples/hello/src/hello.wasl`:

```wasl
use wasi:cli/types;
use wasi:cli/stdout;

world hello {
  import wasi:cli/types;
  import wasi:cli/stdout;

  export wasi:cli/run {
    async func run() -> result {
      local (rx: stream<u8>, tx: stream_writer<u8>) = stream.new();
      local done: future<result<_, types::error_code>> = stdout::write_via_stream(rx);
      stream.write(tx, string.bytes(greeting::text("world")));
      stream.close(tx);
      match (future.read(done)) {
        case ok {
          return result.ok;
        }
        case err {
          return result.err;
        }
      }
    }
  }
}

interface greeting {
  func text(name: string) -> string {
    return "Hello, " + name + "!\n";
  }
}
```

Its WIT:

```wit
package example:hello@0.1.0;

world hello {
  import wasi:cli/types@0.3.0;
  import wasi:cli/stdout@0.3.0;
  export wasi:cli/run@0.3.0;
}
```

```sh
cd examples/hello
weasel run hello                   # fetches wasi:cli, writes wkg.lock (§ 14.2)
# Hello, world!
weasel build hello                 # build/hello.wasm, which wasmtime runs too:
wasmtime run -W gc=y -W function-references=y build/hello.wasm
# Hello, world!
```

What the program shows:

- **An async foreign export.** `run` is an `async func`, as its WIT says, `run: async func() -> result;` (§ 13.12). Its component lifts it with a callback that is never called, and its body blocks where it waits (§ 13.13).
- **A stream, read top to bottom.** The program creates a stream, gives its readable end to the standard output (`rx` moves into `write_via_stream`), writes the bytes, closes the stream, then waits until the standard output is done. `stream.write` waits until the host has taken the bytes: a stream has no buffer (§ 10.12).
- **No `await`.** `stream.write` and `future.read` are async built-in functions: each call waits, and is allowed because `run` is async (§ 2.8, E0416). `write_via_stream` is not async: it only starts the transfer, and its future reports the end.
- **Closing before waiting.** The standard output completes `done` only once the stream is closed. Without `stream.close(tx)`, `tx` would be released at the end of `run`, after `future.read(done)`, which would wait for ever, and the runtime would trap (§ 10.7).
- **The error of the future** is a `types::error_code`, an enum without handles: the `case err` that does not name it releases nothing.

## 15.8 An HTTP service: `hello-http`

The second target program of version 0.4 is a small HTTP service for WASI 0.3, in `examples/hello-http`. `GET /karim` answers `Hello, /karim!`. CI serves it with `weasel serve hello_http` and calls it with `curl`. The wasmtime CLI serves its component too: `wasmtime serve -W gc=y -W function-references=y build/hello_http.wasm`.

`examples/hello-http/wasl.toml`:

```toml
[package]
namespace = "example"
name = "hello-http"
version = "0.1.0"

[dependencies]
"wasi:http" = "0.3.0"
```

`examples/hello-http/src/hello_http.wasl`:

```wasl
use wasi:http/types;

world hello_http {
  import wasi:http/types;

  export wasi:http/handler {
    async func handle(request: types::request) -> result<types::response, types::error_code> {
      local path: string = "/";
      match (request.get_path_with_query()) {
        case some(p) {
          path = p;
        }
        case none {
        }
      }

      local (body: stream<u8>, body_tx: stream_writer<u8>) = stream.new();
      local (trailers: future<result<option<types::trailers>, types::error_code>>,
             trailers_tx: future_writer<result<option<types::trailers>, types::error_code>>) = future.new();
      local (response: types::response, _) =
        types::response::new(types::fields(), option.some(body), trailers);
      task.return(result.ok(response));

      stream.write(body_tx, string.bytes("Hello, " + path + "!\n"));
      stream.close(body_tx);
      future.write(trailers_tx, result.ok(option.none));
    }
  }
}
```

Its WIT:

```wit
package example:hello-http@0.1.0;

world hello-http {
  import wasi:http/types@0.3.0;
  export wasi:http/handler@0.3.0;
}
```

```sh
cd examples/hello-http
weasel serve hello_http &
curl http://127.0.0.1:8080/karim
# Hello, /karim!
```

What the program shows:

- **`task.return`.** The line `task.return(result.ok(response));` cuts the body in two: before it, the handler prepares the response; after it, the host has the response and sends its headers, and the handler writes the body. Writing the body before delivering the response would wait for ever, since the host would not have the body stream yet (§ 8.21). After `task.return`, `return result.err(…);` and `?` are errors (E0418).
- **Tuples of handles.** `stream.new()`, `future.new()` and `types::response::new(…)` return tuples that hold handles, destructured at once (§ 9.4). The future that `response::new` returns, which reports when the body has been sent, is not needed: `_` releases it at once.
- **Moves.** `body` and `trailers` move into `types::response::new`, and `response` into `task.return`. `body_tx` is closed explicitly, and `trailers_tx` moves into `future.write`, so nothing is left to release but `request`, at the end of `handle` (§ 9.9).
- **A future must be written.** `trailers_tx` writes `result.ok(option.none)`, no trailers: releasing it without a value would trap (§ 10.7).
- **One instance per request.** `weasel serve` gives each request its own instance (§ 14.2), so two requests never wait for each other, although the component of 0.4 runs one async task at a time (§ 13.13).

## 15.9 Invalid programs of 0.4

Each line is written in `run` of § 15.7 or in `handle` of § 15.8, unless it says otherwise.

| Code | Error |
|---|---|
| `stream.write(tx, bytes).await;` | E0101: there is no `await`; a call to an async function waits by itself |
| `func run() -> result { … }` in `export wasi:cli/run` | E0218: `run` is an `async func` in WASI 0.3 |
| `func greet(tx: stream_writer<u8>) { stream.write(tx, string.bytes("hi")); }` | E0416: `stream.write` is async, and `greet` is not |
| `func pipe() -> stream_writer<u8>` in an exported interface | E0222: a writable end never appears in the WIT |
| `local (rx: stream<string>, tx: stream_writer<string>) = stream.new();` in `hello` | E0223: no function of the world `hello` carries a `stream<string>` |
| `local p: tuple<stream<u8>, stream_writer<u8>> = stream.new();` | E0220: a tuple that holds handles is destructured at once |
| `local (rx: stream<u8>, tx: stream_writer<u8>) = future.new();` | E0400: `future.new()` returns a `tuple<future<T>, future_writer<T>>` |
| `local (body: stream<u8>) = stream.new();` | E0400: the tuple has two elements |
| `stream.new();` | E0408: nothing gives the type of the ends; destructure them, `local (rx: stream<u8>, tx: stream_writer<u8>) = stream.new();` |
| `if (ready) { task.return(result.ok(response)); }` | E0417: `task.return` is a statement of the top level of the body |
| `task.return(result.ok);` in `run`, written twice | E0417: the result is delivered once |
| `task.return(result.ok(response));` in a function without `async` | E0417: only an async function delivers its result early |
| `return result.err(types::error_code::internal_error(option.none));` after `task.return` | E0418: the response is already delivered; write `return;` |
| `local d: u32 = digits(path)?;` after `task.return`, with `func digits(s: string) -> result<u32, types::error_code>` | E0418: `?` would deliver an error after the response |
| `local lines: stream<u8> = feed::lines();` in a function of the package, where `feed::lines` contains `task.return` and the world does not import `feed` | E0419: a function that delivers early is only called by the host or another component |
| `local await: u32 = 1;` | E0101: `await` is reserved |

## 15.10 The target program of 0.5: `shop`

*Specified, not available yet.*

`examples/shop` is a small shop: two HTTP services and a command that share their types and their logic, the small version of the backend of several services that WaSL aims at. It uses the features of 0.5: locals without type, `for`, bodies without braces, built-in functions with `.`, `==` on records, a type alias, doc comments, `use`, and the standard library: `json`, `http` with its router, and `cli`. 0.5 is done when it runs in the continuous integration.

```
examples/shop/
├── wasl.toml
├── src/
│   ├── shop.wasl       // interface shop: products, orders, totals, JSON
│   ├── catalog.wasl    // world catalog: GET /products, GET /products/{id}
│   ├── orders.wasl     // world orders: POST /orders
│   └── shopctl.wasl    // world shopctl: an order on stdin, its total on stdout
└── tests/
    └── shop_test.wasl  // totals, errors, JSON
```

`examples/shop/wasl.toml` has no dependency: the standard library needs none (§ 3.5).

```toml
[package]
namespace = "example"
name = "shop"
version = "0.1.0"
```

`examples/shop/src/shop.wasl`, the logic, compiled into each of the three components:

```wasl
use wasl:std/json;

/// The shop: its products, its orders, and their JSON. Every world of the
/// project compiles it in: it is a library, not a service.
interface shop {

  /// A price, in cents.
  type cents = u64;

  record product {
    id: u32,
    name: string,
    price: cents,
  }

  record order_line {
    product_id: u32,
    quantity: u32,
  }

  record order {
    customer: string,
    lines: list<order_line>,
  }

  variant order_error {
    empty,
    unknown_product(u32),
    zero_quantity(u32),
  }

  /// The catalog, fixed in the code: WASI has no stable storage yet.
  func products() -> list<product> {
    return [
      product { id: 1, name: "pen", price: 150 },
      product { id: 2, name: "notebook", price: 420 },
      product { id: 3, name: "ink", price: 900 },
    ];
  }

  func find(id: u32) -> option<product> {
    for (p in products()) {
      if (p.id == id)
        return option.some(p);
    }
    return option.none;
  }

  /// The total of an order, or why it is invalid.
  func total(order: order) -> result<cents, order_error> {
    if (list.len(order.lines) == 0)
      return result.err(order_error::empty);
    local sum: cents = 0;
    for (line in order.lines) {
      if (line.quantity == 0)
        return result.err(order_error::zero_quantity(line.product_id));
      match (find(line.product_id)) {
        case some(p) {
          sum = sum + u64(line.quantity) * p.price;
        }
        case none {
          return result.err(order_error::unknown_product(line.product_id));
        }
      }
    }
    return result.ok(sum);
  }

  func describe(e: order_error) -> string {
    match (e) {
      case empty {
        return "the order has no line";
      }
      case unknown_product(id) {
        return "unknown product " + u32.to_string(id);
      }
      case zero_quantity(id) {
        return "zero quantity for product " + u32.to_string(id);
      }
    }
  }

  /// `1205` is written `12.05`.
  func format_cents(c: cents) -> string {
    local rest = c % 100;
    if (rest < 10)
      return u64.to_string(c / 100) + ".0" + u64.to_string(rest);
    return u64.to_string(c / 100) + "." + u64.to_string(rest);
  }

  func product_json(p: product) -> json::value {
    return json::value::object([
      tuple("id", json::from_u64(u64(p.id))),
      tuple("name", json::value::text(p.name)),
      tuple("price", json::from_u64(p.price)),
    ]);
  }

  func products_json() -> json::value {
    local all = products();
    local out: list<mut json::value> = list.filled(list.len(all), json::value::null);
    local i: u32 = 0;
    for (p in all) {
      out[i] = product_json(p);
      i = i + 1;
    }
    return json::value::array(list.freeze(out));
  }

  /// Reads `{"customer": "ada", "lines": [{"product_id": 1, "quantity": 3}]}`.
  func order_from_json(body: string) -> result<order, string> {
    local v = json::parse(body)?;
    local customer = text_of(field(v, "customer")?)?;
    local items = array_of(field(v, "lines")?)?;
    local lines: list<mut order_line> =
      list.filled(list.len(items), order_line { product_id: 0, quantity: 0 });
    local i: u32 = 0;
    for (item in items) {
      lines[i] = order_line {
        product_id: u32(number_of(field(item, "product_id")?)?),
        quantity: u32(number_of(field(item, "quantity")?)?),
      };
      i = i + 1;
    }
    return result.ok(order { customer: customer, lines: list.freeze(lines) });
  }

  func field(v: json::value, name: string) -> result<json::value, string> {
    match (json::field(v, name)) {
      case some(f) {
        return result.ok(f);
      }
      case none {
        return result.err("missing field `" + name + "`");
      }
    }
  }

  func text_of(v: json::value) -> result<string, string> {
    match (json::as_text(v)) {
      case some(s) {
        return result.ok(s);
      }
      case none {
        return result.err("expected a string");
      }
    }
  }

  func array_of(v: json::value) -> result<list<json::value>, string> {
    match (json::as_array(v)) {
      case some(xs) {
        return result.ok(xs);
      }
      case none {
        return result.err("expected an array");
      }
    }
  }

  func number_of(v: json::value) -> result<u64, string> {
    match (json::as_u64(v)) {
      case some(n) {
        return result.ok(n);
      }
      case none {
        return result.err("expected a whole number");
      }
    }
  }

}
```

`examples/shop/src/catalog.wasl`:

```wasl
use wasl:std/http;
use wasl:std/json;

/// The catalog service: `GET /products` and `GET /products/{id}`.
/// Run it with `weasel serve catalog`, then `curl http://127.0.0.1:8080/products/2`.
world catalog {
  export wasl:std/http {
    async func handle(request: request) -> response {
      local r = router(request);
      if (r.get("/products"))
        return reply(200, "application/json", json::stringify(shop::products_json()));
      if (r.get("/products/{id}"))
        return catalog_api::product(r.param("id"));
      return r.unsupported();
    }
  }
}

interface catalog_api {

  func product(id: string) -> http::response {
    match (find(id)) {
      case some(p) {
        return http::reply(200, "application/json", json::stringify(shop::product_json(p)));
      }
      case none {
        return http::reply(404, "text/plain", "no product " + id + "\n");
      }
    }
  }

  func find(id: string) -> option<shop::product> {
    return shop::find(u32.parse(id)?);
  }

}
```

`examples/shop/src/orders.wasl`:

```wasl
use wasl:std/http;

/// The orders service: `POST /orders` with an order in JSON answers its total.
/// Run it with `weasel serve orders`, then
/// `curl -d '{"customer": "ada", "lines": [{"product_id": 1, "quantity": 3}]}' http://127.0.0.1:8080/orders`.
world orders {
  export wasl:std/http {
    async func handle(request: request) -> response {
      local r = router(request);
      if (r.post("/orders"))
        return checkout::quote(request);
      return r.unsupported();
    }
  }
}

interface checkout {

  func quote(request: http::request) -> http::response {
    match (read_order(request)) {
      case ok(order) {
        match (shop::total(order)) {
          case ok(cents) {
            return http::reply(200, "application/json", "{\"total\":" + u64.to_string(cents) + "}\n");
          }
          case err(e) {
            return http::reply(422, "text/plain", shop::describe(e) + "\n");
          }
        }
      }
      case err(message) {
        return http::reply(400, "text/plain", message + "\n");
      }
    }
  }

  func read_order(request: http::request) -> result<shop::order, string> {
    match (http::body_text(request)) {
      case some(body) {
        return shop::order_from_json(body);
      }
      case none {
        return result.err("the body is not UTF-8");
      }
    }
  }

}
```

`examples/shop/src/shopctl.wasl`:

```wasl
use wasl:std/cli;

/// Reads an order in JSON on the standard input and prints its total:
/// `echo '{"customer": "ada", "lines": [{"product_id": 1, "quantity": 3}]}' | weasel run shopctl`.
world shopctl {
  export wasl:std/cli {
    async func run() -> result<_, string> {
      local order = shop::order_from_json(read_all()?)?;
      match (shop::total(order)) {
        case ok(cents) {
          print(order.customer + ": " + shop::format_cents(cents) + "\n");
          return result.ok;
        }
        case err(e) {
          return result.err(shop::describe(e));
        }
      }
    }
  }
}
```

`examples/shop/tests/shop_test.wasl`:

```wasl
interface shop_test {

  func total_of_two_lines() -> result<_, string> {
    local order = shop::order {
      customer: "ada",
      lines: [
        shop::order_line { product_id: 1, quantity: 3 },
        shop::order_line { product_id: 3, quantity: 1 },
      ],
    };
    if (shop::total(order) != result.ok(1350))
      return result.err("expected a total of 1350");
    return result.ok;
  }

  func empty_order() -> result<_, string> {
    local order = shop::order { customer: "ada", lines: [] };
    if (shop::total(order) != result.err(shop::order_error::empty))
      return result.err("expected `empty`");
    return result.ok;
  }

  func unknown_product() -> result<_, string> {
    local order = shop::order {
      customer: "ada",
      lines: [shop::order_line { product_id: 9, quantity: 1 }],
    };
    if (shop::total(order) != result.err(shop::order_error::unknown_product(9)))
      return result.err("expected `unknown_product(9)`");
    return result.ok;
  }

  func order_from_json() -> result<_, string> {
    local order = shop::order_from_json(
      "{\"customer\": \"ada\", \"lines\": [{\"product_id\": 2, \"quantity\": 2}]}",
    )?;
    local expected = shop::order {
      customer: "ada",
      lines: [shop::order_line { product_id: 2, quantity: 2 }],
    };
    if (order != expected)
      return result.err("unexpected order");
    return result.ok;
  }

  func missing_field() -> result<_, string> {
    match (shop::order_from_json("{\"customer\": \"ada\"}")) {
      case ok {
        return result.err("expected an error");
      }
      case err(message) {
        if (message != "missing field `lines`")
          return result.err("unexpected message: " + message);
        return result.ok;
      }
    }
  }

  func format_cents() -> result<_, string> {
    if (shop::format_cents(1205) != "12.05")
      return result.err("expected 12.05, got " + shop::format_cents(1205));
    if (shop::format_cents(90) != "0.90")
      return result.err("expected 0.90, got " + shop::format_cents(90));
    return result.ok;
  }

}
```

The continuous integration runs, in `examples/shop`:

```sh
weasel fmt --check
weasel build --all          # build/catalog.wasm, build/orders.wasm, build/shopctl.wasm
weasel test                 # 6 passed

weasel serve catalog --addr 127.0.0.1:8081 &
curl -s http://127.0.0.1:8081/products/2
# {"id":2,"name":"notebook","price":420}
curl -s -o /dev/null -w '%{http_code}' -X DELETE http://127.0.0.1:8081/products/2
# 405

weasel serve orders --addr 127.0.0.1:8082 &
curl -s -d '{"customer": "ada", "lines": [{"product_id": 1, "quantity": 3}]}' http://127.0.0.1:8082/orders
# {"total":450}

echo '{"customer": "ada", "lines": [{"product_id": 3, "quantity": 2}]}' | weasel run shopctl
# ada: 18.00
```

What the program shows:

- **One logic, three components.** `shop` is exported by no world: each component compiles the part it reaches (§ 2.3). `json::value` appears in its signatures, which is possible because it never reaches a WIT (§ 7.11).
- **Entry points without WASI.** The three worlds export `wasl:std/http` or `wasl:std/cli`, and write only `handle` or `run`; the built files are WASI 0.3 services and commands (§ 17.1).
- **The routes are a table.** `handle` only chooses a function; the work is in `catalog_api` and `checkout`, which tests could call. A route that needs a helper puts it in an interface, since the block of a foreign export only holds the functions without body (§ 2.4.3).
- **What stays verbose.** Converting JSON to a record takes a helper per JSON type, to turn an `option` into a `result` with a message, and building a list from another one takes a `list<mut T>` and an index. These are the places where function values, a growable list, or code generated from records would help (§ 16).

## 15.11 Invalid programs of 0.5

Each line is written in `examples/shop` (§ 15.10), unless it says otherwise.

| Code | Error |
|---|---|
| `local n = list::len(lines);` | E0100: a built-in function is written `list.len(lines)` |
| `local n = lines.len();` | E0100: a built-in function is not a method; write `list.len(lines)` |
| `return status.paid;` in `examples/invoice` | E0300: `status` is a type, not a value; a declared name is written `status::paid` |
| `local x;` | E0100: a local needs a type or an initial value |
| `local found = option.none;` | E0408: nothing gives the type of the option; write `local found: option<shop::product> = option.none;` |
| `local xs = [];` | E0408: nothing gives the element type |
| `if (a) if (b) f();` | E0100: a body without braces is a simple statement |
| `if (ok) local n = 1;` | E0100: a `local` alone in a body would be useless; write braces |
| `for (c in name) { }` with `name: string` | E0420: `for` goes through lists; write `string.chars(name)` |
| `for (i in 0..10) { }` | E0100: there are no ranges; write a `while` |
| `_ = shop::total(order);` | E0100: there is no `_ =`; write `shop::total(order);` |
| `json::parse(body)` in a file without `use wasl:std/json;` | E0300: `json` is not named in this file; add `use wasl:std/json;` |
| `use wasl:std/json;` after `world catalog { … }` | E0100: the `use` items come first |
| `use wasl:std/nope;` | E0215: `wasl:std` has no interface `nope` |
| `use wasl:std/http;` with an `interface http` in the package | E0217: `http` already names `wasl:std/http`; write `use wasl:std/http as std_http;` |
| `type cents = cents;` | E0225: an alias cannot name itself |
| `func quote(request: http::request) -> http::response;` in `checkout` | E0224: a function of the package needs a body |
| `async func handle(request: request) -> response { … }` and `func reply(…) { … }` in `export wasl:std/http` | E0218: the block only writes `handle`; `reply` belongs to the library |
| `export wasl:std/http { }` | E0218: `handle` is missing |
| `func handle(request: request) -> response { … }` in `export wasl:std/http` | E0218: `handle` is an `async func` |
| `if (order == other)` with a type that has a `mut` field | E0403: a value that can change has no equality |
| `func show(c: shop::cents) { cli::print(…); }` | E0416: `print` is async, and `show` is not |
| `world helpers { }` in `tests/shop_test.wasl` | E0226: a test file only declares interfaces |
| `"wasl:std" = "0.5.0"` in `[dependencies]` | E0200: the standard library is not declared |
