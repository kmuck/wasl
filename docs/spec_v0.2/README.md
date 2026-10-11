# WaSL v0.2 Specification

Status: **accepted, provisional, implemented by `waslc` 0.2**. This version is the reference for v0.2, but it is not final before v0.3: it may still change with what the implementation of the compiler teaches. This version is written from its target program, [`examples/invoice`](../../examples/invoice), following the example-driven rule of § 16.1 of v0.1; `waslc` 0.2 compiles it and the continuous integration runs it.

Version 0.2 adds **data on the WasmGC heap**, and with it every WIT type except resources and async types: `char`, `string`, `list<T>`, `tuple<…>`, `option<T>`, `result<T, E>`, `record`, `enum`, `variant` and `flags`. Values of these types live in the garbage-collected heap of the runtime, are shared by reference inside a component, and are copied through linear memory only when they cross the boundary of a component.

```wasl
world billing {
  export invoices;
}

interface invoices {

  record line {
    label: string,
    quantity: u32,
    unit_price: u64,
  }

  enum invoice_error {
    no_lines,
    zero_quantity,
  }

  func total(lines: list<line>) -> result<u64, invoice_error> {
    if (list::len(lines) == 0) {
      return result::err(invoice_error::no_lines);
    }
    local sum: u64;
    local i: u32;
    while (i < list::len(lines)) {
      if (lines[i].quantity == 0) {
        return result::err(invoice_error::zero_quantity);
      }
      sum = sum + u64(lines[i].quantity) * lines[i].unit_price;
      i = i + 1;
    }
    return result::ok(sum);
  }

}
```

```sh
weasel call invoices::total '[{label: "pen", quantity: 3, unit-price: 150}]'    # → ok(450)
weasel call invoices::total '[]'                                                # → err(no-lines)
```

## How this specification is organized

v0.2 **extends** [v0.1](../spec_v0.1/README.md). Everything v0.1 specifies still holds unless a chapter of this document amends it, and section references without a version (`§ 8.2`) designate v0.1. References to this document are written `§ v2.4.3`. When v0.2 is implemented, the two documents are merged into a single complete specification, as v0.1 is today.

| § | Chapter | Content |
|---|---|---|
| v2.1 | [Overview](01-overview.md) | Scope, design decisions, what changes for v0.1 programs |
| v2.2 | [Types](02-types.md) | The new types, type declarations in interfaces, recursion |
| v2.3 | [Syntax](03-syntax.md) | New tokens and keywords, complete grammar of v0.2 |
| v2.4 | [Typing](04-typing.md) | Literals, constructors, field access, indexing, `match`, operators, built-in functions |
| v2.5 | [Runtime semantics](05-semantics.md) | References, aliasing, mutability, evaluation order, traps |
| v2.6 | [Mapping to WasmGC](06-wasmgc-mapping.md) | Representation of each type, generated code |
| v2.7 | [Component boundary](07-boundary.md) | Generated WIT, Canonical ABI, copies through linear memory |
| v2.8 | [Diagnostics](08-diagnostics.md) | New error codes, order of analysis |
| v2.9 | [Tooling](09-tooling.md) | `weasel call` with WAVE values |
| v2.10 | [Examples](10-examples.md) | The target program, invalid programs |
| v2.11 | [Open questions](11-open-questions.md) | What this draft leaves undecided |

## Conventions

Those of v0.1 (§ 1, *Conventions* of its README). In addition, a **scalar type** is a v0.1 type, `char`, an enum or a flags type, and a **heap type** is any other type introduced by v0.2 (§ v2.2.1).
