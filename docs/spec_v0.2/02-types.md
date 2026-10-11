# v2.2 Types

## v2.2.1 Overview

v0.2 adds ten kinds of types to the eleven primitive types of § 7.1. They are exactly the WIT types of the same name.

| Type | Values | Declared | WasmGC representation (chapter v2.6) |
|---|---|---|---|
| `char` | a Unicode scalar value | built in | `i32` |
| `string` | an immutable sequence of Unicode scalar values, stored as UTF-8 | built in | `(ref $bytes)`, an array of bytes |
| `list<T>`, `list<mut T>` | a sequence of a fixed number of values of type `T` | built in | `(ref $list_T)`, an array |
| `tuple<T1, …, Tn>` | a fixed group of unnamed values | built in | `(ref $tuple_…)`, a struct |
| `option<T>` | `none`, or `some` with a value of type `T` | built in | a nullable reference |
| `result<T, E>` | `ok` with a value of type `T`, or `err` with a value of type `E` | built in | a variant with two cases |
| `record r { … }` | a group of named fields | in an interface | `(ref $r)`, a struct |
| `enum e { … }` | one of a list of named cases | in an interface | `i32` |
| `variant v { … }` | one of a list of named cases, each with an optional payload | in an interface | `(ref $v)`, a struct per case |
| `flags f { … }` | a set of named flags | in an interface | `i32`, one bit per flag |

Categories used by the rules of this document:

- **scalar types**: the primitive types of v0.1, `char`, enums and flags. They are values, represented by a Wasm number;
- **heap types**: every other type of this table. Their values are references to objects of the WasmGC heap (§ v2.5.1);
- **case types**: enums, variants, options and results, the types a `match` inspects (§ v2.4.9).

As in v0.1, all types are distinct (§ 7.3): two records with the same fields are two different types, `list<T>` and `list<mut T>` are two different types, and no type converts implicitly to another.

## v2.2.2 Built-in types

### `char`

- A `char` is a Unicode scalar value: a code point from `U+0000` to `U+10FFFF`, surrogates (`U+D800` to `U+DFFF`) excluded. Every `char` value is valid.
- It is written as a literal, `'a'`, `'é'`, `'\n'`, `'\u{1F600}'` (§ v2.3.1), compared with `==`, `!=`, `<`, `<=`, `>`, `>=` in code point order (§ v2.4.10), and converted to its code point with `u32(c)` (§ v2.4.11).
- It is a scalar: a local of type `char` without initial value is `'\u{0}'`.

### `string`

- A string is an immutable sequence of Unicode scalar values, encoded in UTF-8. Its length (`string::len`) is its number of **bytes**.
- A string is always valid UTF-8: every way to build one (literals, `+`, `string::from_bytes`, `string::from_chars`, the component boundary) guarantees it.
- Strings are compared by content with `==` and `!=`, and concatenated with `+` (§ v2.4.10).

### `list<T>` and `list<mut T>`

- A list has a **fixed length**, chosen when it is created, like a WebAssembly array: it never grows nor shrinks (§ v2.6.1).
- A `list<T>` is **immutable**: once it is created, none of its elements ever changes, whoever holds it. A `list<mut T>` also allows `xs[i] = e;`. The two are distinct types (§ v2.2.6).
- `T` is any type, including another list: `list<list<u8>>`. `mut` only applies to the elements of the list it is written on: in `list<mut list<u8>>` the outer list is mutable, the inner ones are not.

### `tuple<T1, …, Tn>`

- A tuple groups one or more values of fixed types, without names: `tuple<u32, u64>`.
- It is created with `tuple(e1, …, en)` and its elements are read with `t.0`, `t.1`… (§ v2.4.6). Its elements are immutable, as in a record without `mut` field.

### `option<T>`

`option<T>` has two cases: `none`, without payload, and `some`, with a payload of type `T`. `T` is any type, including another option.

### `result<T, E>`

`result` has two cases, `ok` and `err`. Each one has a payload or not, which gives four forms, exactly those of WIT:

| Form | `ok` | `err` |
|---|---|---|
| `result<T, E>` | payload `T` | payload `E` |
| `result<T>` | payload `T` | no payload |
| `result<_, E>` | no payload | payload `E` |
| `result` | no payload | no payload |

## v2.2.3 Declared types

`record`, `enum`, `variant` and `flags` types are declared **inside an interface**, next to its functions, exactly as in WIT.

```wasl
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
    status: mut status,
  }

  func total(inv: invoice) -> result<u64, invoice_error> { … }

}
```

In each kind of declaration, the fields, cases or flags are separated by commas; a trailing comma is allowed. There is at least one, and two of them cannot have the same name (E0207).

### `record`

- A record has one or more **fields**, each with a name and a type.
- A field declared `mut` can be assigned (`inv.status = status::paid;`); a field without `mut` is set once, when the record is created. This is exactly the mutability of WasmGC struct fields (§ v2.6.1).

### `enum`

An enum has one or more **cases**, names without payload.

### `variant`

A variant has one or more cases, each with an optional payload of **one** type: `percent(u8)`. To carry several values, the payload is a record or a tuple.

### `flags`

- A flags type has one to 32 **flags** (E0212 beyond, the limit of the Component Model). A value is a set of these flags: any combination, including none or all of them.
- It is written as a literal listing the flags it contains, `delivery { email, urgent }`, or `delivery { }` for the empty set; `f.paper` tells whether a flag is in the set; `|` and `&` give the union and the intersection (§ v2.4.10).
- Flags values are scalars, like integers: they have no identity and are copied, never shared. A local of a flags type without initial value is the empty set.

### Names

- The names of the types and functions of an interface form **one namespace**, as in WIT: a type and a function of the same interface cannot have the same name (E0206). Two functions with the same name remain E0202.
- A type cannot have the name of a top-level declaration of the package (a world, an interface or a global, E0210). This keeps every path unambiguous: in `a::b`, `a` is either a type of the current interface or an interface, never both.
- Field, case and flag names live in their type. A field can have the name of a type (`status: mut status`).
- Every name follows the identifier rules of § 4.3, and converts to WIT by replacing `_` with `-` (§ 13.2): `unit_price` → `unit-price`, `invoice_error` → `invoice-error`.

## v2.2.4 Naming a type

| Where | How | Example |
|---|---|---|
| in the interface that declares it | its name | `line` |
| in another interface | the interface, then the name | `invoices::line` |
| in the type of a global | the interface, then the name | `global first: invoices::line = …;` |

There is only one level of path, as for function calls (§ 9.1). Using an unknown type, or naming a function, an interface or a world where a type is expected, is an error (E0305).

The built-in types are keywords and are written the same everywhere: `string`, `list<u8>`, `option<invoices::line>`, `tuple<char, u32>`.

## v2.2.5 Recursive types

A record or variant can refer to itself, directly or through other types:

```wasl
interface tree {
  record node {
    value: u32,
    children: list<node>,
  }
}
```

- Every declared type must have at least one value that a program can build; otherwise the declaration is an error (E0211). A type **has a value** when:
  - a scalar, `string`, `list<T>` or `option<T>`: always (a list can be empty, an option can be `none`);
  - a `record` or a `tuple`: all its fields or elements have a value;
  - a `variant`: at least one of its cases has a value, a case without payload always having one; a `result` likewise (`result<a>` has the value `err`);
  - an `enum` or a `flags` type: always.

  Types that contain themselves are judged by this rule, computed from the types that obviously have a value. `variant expr { lit(u32), neg(expr) }` has a value (`lit(1)`, then `neg(lit(1))`), so it is valid. `record a { next: a }`, `record b { pair: tuple<u32, b> }` and `variant v { a(v), b(tuple<u32, v>) }` have none: building one would already need one. The error names the first field without a value of a record, and suggests the fixes: a case without payload, or an `option` or a `list` in the cycle.
- The Component Model does not allow recursive types. A recursive type can therefore only be used **inside** a component: using it, directly or through another type, in the signature of an exported function is an error (E0208).

## v2.2.6 Immutable and mutable lists

`list<T>` and `list<mut T>` are two distinct types. Neither can be used where the other is expected (E0400); going from one to the other always creates a new list:

| From | To | With |
|---|---|---|
| `list<mut T>` | `list<T>` | `list::freeze(xs)`: a new immutable list with the elements of `xs` |
| `list<T>` or `list<mut T>` | `list<mut T>` | `list::copy(xs)`: a new mutable list with the elements of `xs` |

```wasl
func digits() -> list<u8> {
  local d: list<mut u8> = list::filled(3, 0);
  d[0] = 1;
  return list::freeze(d);    // `return d;` would be E0400
}
```

Why immutability is a guarantee and not a read-only view:

- **A `list<T>` can be trusted.** A function that receives or stores a `list<T>` knows that its elements will never change, whatever the rest of the program does. Only `list<mut T>` values can be shared and modified at the same time.
- **It prepares copy-free boundaries.** With the `gc` option of the Canonical ABI (§ v2.11), an immutable list can be handed to another component by reference, since nobody can modify it; a mutable one must still be copied. The language must therefore know, for every list, that it is really immutable.

Most lists are built once, with a literal, `list::filled` or at the boundary, and need no `freeze`. The copy made by `list::freeze` is the price of building an immutable list element by element; the representation that makes it necessary today is explained in § v2.6.1.

## v2.2.7 Types in signatures

Any type can be used for parameters, return values, locals, globals, fields, list and tuple elements and payloads. The restrictions only concern what appears in the WIT of a world: the signatures of **exported** functions, and every type declared in an **exported interface**, which appears in the WIT even if no function uses it (§ v2.7.1):

- no recursive type (E0208, § v2.2.5);
- every type they use must be declared in an interface that can appear in the WIT of the world (E0209, § v2.7.1).
