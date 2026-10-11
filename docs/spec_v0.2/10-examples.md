# v2.10 Examples

## v2.10.1 The target program

This project is in [`examples/invoice`](../../examples/invoice). It was written before this specification, as the program v0.2 must compile (§ 16.1), and every rule of chapters v2.2 to v2.7 was checked against it.

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
    status: mut status,
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

  // `inv` is a copy made at the boundary of the component: changing its
  // status changes neither the caller's value nor anything else.
  func mark_paid(inv: invoice) -> result<invoice, invoice_error> {
    match (total(inv)) {
      case ok {
        inv.status = status::paid;
        return result::ok(inv);
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

- `invoices` declares five types next to its functions, as a WIT interface does; `pricing` names them with a path (`invoices::discount`).
- `line` and `invoice` are records; `status` is the only `mut` field, so `mark_paid` can change it and nothing else.
- `discount` is a variant whose cases both carry a payload; `invoice.discount` is optional.
- `total` returns a `result`; `match` is the only way to look inside an option, a result or a variant.
- `company` is a global string, initialized with a constant (§ v2.4.12).
- `find` compares strings with `==` and returns an `option`; `count_unpaid` compares enums with `!=`.
- `text::from_u32` builds a string from a `list<mut u8>` with the built-in functions only: number formatting belongs to a library, not to the compiler.
- `mark_paid` changes the status of its argument and returns it: the caller's value does not change, because the argument is a copy made at the boundary (§ v2.5.4).

WIT of the package (`weasel wit`):

```wit
package example:invoice@0.1.0;

interface invoices {
  enum status {
    draft,
    sent,
    paid,
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
{number: 42, customer: "ACME", lines: [{label: "pen", quantity: 3, unit-price: 150}, {label: "notebook", quantity: 2, unit-price: 425}], discount: some(percent(10)), status: draft}
$ INV='{number: 42, customer: "ACME", lines: [{label: "pen", quantity: 3, unit-price: 150}, {label: "notebook", quantity: 2, unit-price: 425}], discount: some(percent(10)), status: draft}'
$ weasel call invoices::total "$INV"
ok(1170)
$ weasel call invoices::summary "$INV"
"Weasel & Co: invoice 42 for ACME (draft)"
$ weasel call invoices::mark_paid "$INV"
ok({number: 42, customer: "ACME", lines: [{label: "pen", quantity: 3, unit-price: 150}, {label: "notebook", quantity: 2, unit-price: 425}], discount: some(percent(10)), status: paid})
$ weasel call invoices::total '{number: 7, customer: "", lines: [], discount: none, status: draft}'
err(empty-customer)
$ weasel call invoices::total '{number: 7, customer: "Zed", lines: [{label: "x", quantity: 1, unit-price: 100}], discount: some(fixed(500)), status: sent}'
err(discount-too-large)
$ weasel call invoices::count_unpaid "[$INV]"
1
$ weasel call invoices::find "[$INV]" '"Zed"'
none
```

## v2.10.2 Invalid programs

Each line is placed in a valid context, with the types of the target program, and produces the error shown.

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
| `local s: status;` | E0410: an enum needs an initial value |
| `match (n) { … }` with `n: u32` | E0411: use `if` on numbers |
| `inv.number = 7;` | E0505: `number` is not `mut` |
| `inv.lines[0] = l;` | E0505: `lines` is a `list<line>`, not a `list<mut line>` |
| `match (s) { case draft { } case sent { } }` | E0506: `paid` is not covered |
| `match (s) { case draft { } case sent { } case paid { } else { } }` | E0507: the `else` covers no case |
| `if (inv == other) { }` | E0403: records cannot be compared with `==` |
| `if (inv.customer < "M") { }` | E0403: strings have no order |
| `local c: u8 = inv.customer[0];` | E0403: strings cannot be indexed; use `string::bytes(inv.customer)[0]` |
| `local xs: list<mut u8> = string::bytes(s);` | E0400: a `list<u8>` is not a `list<mut u8>`; use `list::copy` |
| `return paid;` | E0300: write `status::paid` |
| `f().number = 1;` | E0100: an assignment target starts with a name |
