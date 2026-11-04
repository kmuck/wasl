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
      discount: option::some(discount::percent(10)),
      delivery: delivery { email, urgent },
      status: status::draft,
    };
  }

  // Total in cents, after the discount.
  func total(inv: invoice) -> result<u64, invoice_error> {
    if (string::len(inv.customer) == 0) {
      return result::err(invoice_error::empty_customer);
    }
    if (list::len(inv.lines) == 0) {
      return result::err(invoice_error::no_lines);
    }
    local sum: u64;
    local i: u32;
    while (i < list::len(inv.lines)) {
      local l: line = inv.lines[i];
      if (l.quantity == 0) {
        return result::err(invoice_error::zero_quantity);
      }
      sum = sum + u64(l.quantity) * l.unit_price;
      i = i + 1;
    }
    match (inv.discount) {
      case some(d) {
        return pricing::apply(sum, d);
      }
      case none {
        return result::ok(sum);
      }
    }
  }

  // An invoice crosses the boundary of the component, so it cannot change
  // (§ 7.11): a paid invoice is a new record.
  func mark_paid(inv: invoice) -> result<invoice, invoice_error> {
    match (total(inv)) {
      case ok {
        return result::ok(invoice {
          number: inv.number,
          customer: inv.customer,
          lines: inv.lines,
          discount: inv.discount,
          delivery: inv.delivery,
          status: status::paid,
        });
      }
      case err(e) {
        return result::err(e);
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
    local chars: list<char> = string::chars(inv.customer);
    if (list::len(chars) == 0) {
      return option::none;
    }
    return option::some(chars[0]);
  }

  // Number of invoices, and sum of the totals that are valid.
  func stats(all: list<invoice>) -> tuple<u32, u64> {
    local sum: u64;
    local i: u32;
    while (i < list::len(all)) {
      match (total(all[i])) {
        case ok(t) {
          sum = sum + t;
        }
        case err {
        }
      }
      i = i + 1;
    }
    return tuple(list::len(all), sum);
  }

  func find(all: list<invoice>, customer: string) -> option<invoice> {
    local i: u32;
    while (i < list::len(all)) {
      if (all[i].customer == customer) {
        return option::some(all[i]);
      }
      i = i + 1;
    }
    return option::none;
  }

  func count_unpaid(all: list<invoice>) -> u32 {
    local count: u32;
    local i: u32;
    while (i < list::len(all)) {
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
          return result::err(invoices::invoice_error::discount_too_large);
        }
        return result::ok(sum - sum * u64(p) / 100);
      }
      case fixed(amount) {
        if (amount > sum) {
          return result::err(invoices::invoice_error::discount_too_large);
        }
        return result::ok(sum - amount);
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
    local bytes: list<mut u8> = list::filled(count, 0);
    rest = n;
    while (count > 0) {
      count = count - 1;
      bytes[count] = u8(rest % 10) + 48;
      rest = rest / 10;
    }
    match (string::from_bytes(bytes)) {
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
- `initial` decodes the customer's name with `string::chars` and returns an `option<char>`; `stats` returns two values in a `tuple<u32, u64>`.
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
$ weasel call invoices::sample
{number: 42, customer: "ACME", lines: [{label: "pen", quantity: 3, unit-price: 150}, {label: "notebook", quantity: 2, unit-price: 425}], discount: some(percent(10)), delivery: {email, urgent}, status: draft}
$ INV='{number: 42, customer: "ACME", lines: [{label: "pen", quantity: 3, unit-price: 150}, {label: "notebook", quantity: 2, unit-price: 425}], discount: some(percent(10)), delivery: {email, urgent}, status: draft}'
$ weasel call invoices::total "$INV"
ok(1170)
$ weasel call invoices::summary "$INV"
"Weasel & Co: invoice 42 for ACME (draft)"
$ weasel call invoices::mark_paid "$INV"
ok({number: 42, customer: "ACME", lines: [{label: "pen", quantity: 3, unit-price: 150}, {label: "notebook", quantity: 2, unit-price: 425}], discount: some(percent(10)), delivery: {email, urgent}, status: paid})
$ weasel call invoices::total '{number: 7, customer: "", lines: [], discount: none, delivery: {}, status: draft}'
err(empty-customer)
$ weasel call invoices::total '{number: 7, customer: "Zed", lines: [{label: "x", quantity: 1, unit-price: 100}], discount: some(fixed(500)), delivery: {paper}, status: sent}'
err(discount-too-large)
$ weasel call invoices::count_unpaid "[$INV]"
1
$ weasel call invoices::initial "$INV"
some('A')
$ weasel call invoices::needs_printing "$INV"
false
$ weasel call invoices::stats "[$INV, $INV]"
(2, 2340)
$ weasel call invoices::find "[$INV]" '"Zed"'
none
```

## 15.4 Invalid programs

Each line is a statement or a declaration placed in a valid context, with the error it produces.

### With numbers

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
| `fib(3);` | E0413: the value of `fib` is ignored; write `_ = fib(3);` |
| `break;` outside a loop | E0501 |
| `return;` in `func f() -> u32` | E0502 |
| `return 1;` in `func f()` | E0503 |
| `func f(x: u32) -> u32 { if (x > 0) { return x; } }` | E0504: no value returned when `x == 0` |
| `global g: u32 = 1 + 1;` | E0409: the initial value of a global is a constant expression |
| `interface i { global g: u32 = 0; }` | E0100: globals are top-level declarations |
| `world w { export nope; }` | E0203: unknown interface |
| `world w { export wasi:cli/run@0.3.0 { … } }` | E0001: `@` is not a valid character; exporting a foreign interface is not available yet (§ 2.4.2) |
| `world w { export wasi:cli/run { … } }` | E0100: only the name of an interface of the package can follow `export` |
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
| `func sort(xs: list<mut u32>)` in `invoices` | E0214: use `list<u32>`, and `list::copy` inside the function |
| `local x: lines = …;` | E0305: unknown type |
| `local l: line = …;` in `pricing` | E0305: from another interface, write `invoices::line` |
| `inv.total` | E0306: `invoice` has no field `total` |
| `status::lost` | E0307: `status` has no case `lost` |
| `line { quantity: 1, label: "pen", unit_price: 5 }` | E0406: fields in declaration order |
| `line { label: "pen", quantity: 1 }` | E0406: missing field `unit_price` |
| `status::paid()` | E0407: `paid` has no payload |
| `discount::percent` | E0407: `percent` needs a payload |
| `case none(x) { }` | E0407: `none` has no payload |
| `local d: u8 = option::none;` | E0400: `option::none` is not a `u8` |
| `match (option::none) { … }` | E0408: no type for `option::none` |
| `local n: u32 = list::len([]);` | E0408: no element type for `[]` |
| `global s: string = "a" + "b";` | E0409: an operator is not a constant expression |
| `local s: string;` | E0410: a string needs an initial value |
| `local m: line;` | E0410: a record needs an initial value |
| `total(inv);` | E0413: the `result` of `total` is ignored; handle it, or write `_ = total(inv);` |
| `_ = n;` | E0100: only the value of a call can be discarded |
| `match (n) { … }` with `n: u32` | E0411: use `if` on numbers |
| `inv.number = 7;` | E0505: `number` is not `mut` |
| `inv.status = status::paid;` | E0505: `status` is not `mut`; build a new `invoice` |
| `inv.lines[0] = l;` | E0505: `lines` is a `list<line>`, not a `list<mut line>` |
| `match (s) { case draft { } case sent { } }` | E0506: `paid` is not covered |
| `match (s) { case draft { } case sent { } case paid { } else { } }` | E0507: the `else` covers no case |
| `if (inv == other) { }` | E0403: records cannot be compared with `==` |
| `if (inv.customer < "M") { }` | E0403: strings have no order |
| `local c: u8 = inv.customer[0];` | E0403: strings cannot be indexed; use `string::bytes(inv.customer)[0]` |
| `local xs: list<mut u8> = string::bytes(s);` | E0400: a `list<u8>` is not a `list<mut u8>`; use `list::copy` |
| `return d;` with `d: list<mut u8>` in `func f() -> list<u8>` | E0400: a `list<mut u8>` is not a `list<u8>`; use `list::freeze(d)` |
| `local c: char = '';` | E0007: a char literal holds exactly one character |
| `local c: char = char(n);` | E0404: use `char::from_u32(n)`, which can fail |
| `local n: u64 = u64(c);` with `c: char` | E0404: a `char` only converts to `u32` |
| `local d: delivery = delivery { urgent, email };` | E0412: flags in declaration order |
| `inv.delivery.paper = true;` | E0505: write `d = d \| delivery { paper };` on a local |
| `local t: tuple<u32, u64> = tuple(1, 2, 3);` | E0400: three elements for a tuple of two |
| `stats(all).2` | E0306: the tuple has two elements |
| `return paid;` | E0300: write `status::paid` |
| `f().number = 1;` | E0100: an assignment target starts with a name |
