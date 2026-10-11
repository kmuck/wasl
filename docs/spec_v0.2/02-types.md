# v2.2 Types

## v2.2.1 Overview

v0.2 adds seven kinds of types to the eleven primitive types of § 7.1. They are exactly the WIT types of the same name.

| Type | Values | Declared | WasmGC representation (chapter v2.6) |
|---|---|---|---|
| `string` | an immutable sequence of Unicode scalar values, stored as UTF-8 | built in | `(ref $bytes)`, an array of bytes |
| `record r { … }` | a group of named fields | in an interface | `(ref $r)`, a struct |
| `enum e { … }` | one of a list of named cases | in an interface | `i32` |
| `variant v { … }` | one of a list of named cases, each with an optional payload | in an interface | `(ref $v)`, a struct per case |
| `list<T>`, `list<mut T>` | a sequence of a fixed number of values of type `T` | built in | `(ref $list_T)`, an array |
| `option<T>` | `none`, or `some` with a value of type `T` | built in | a nullable reference |
| `result<T, E>` | `ok` with a value of type `T`, or `err` with a value of type `E` | built in | a variant with two cases |

Categories used by the rules of this document:

- **scalar types**: the primitive types of v0.1 and the enums;
- **heap types**: every other type of this table;
- **types with payload cases**: `variant`, `option` and `result`; together with `enum`, they are the **case types**, the types a `match` inspects (§ v2.4.9).

As in v0.1, all types are distinct (§ 7.3): two records with the same fields are two different types, and no type converts implicitly to another. The only exception is the read-only view of a mutable list (§ v2.2.6).

## v2.2.2 Built-in types

### `string`

- A string is an immutable sequence of Unicode scalar values, encoded in UTF-8. Its length (`string::len`) is its number of **bytes**.
- A string is always valid UTF-8: every way to build one (literals, `+`, `string::from_bytes`, the component boundary) guarantees it.
- Strings are compared by content with `==` and `!=`, and concatenated with `+` (§ v2.4.10).

### `list<T>` and `list<mut T>`

- A list has a **fixed length**, chosen when it is created, like a WebAssembly array. Growable sequences (`vec<T>`) come later (chapter 16).
- `list<T>` gives read-only access to its elements; `list<mut T>` also allows `xs[i] = e;`.
- `T` is any type, including another list: `list<list<u8>>`. `mut` only applies to the elements of the list it is written on: in `list<mut list<u8>>` the outer list is mutable, the inner ones are not.

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

`record`, `enum` and `variant` types are declared **inside an interface**, next to its functions, exactly as in WIT.

```wasl
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

  func total(inv: invoice) -> result<u64, invoice_error> { … }

}
```

### `record`

- A record has one or more **fields**, each with a name and a type, separated by commas; a trailing comma is allowed.
- A field declared `mut` can be assigned (`inv.status = status::paid;`); a field without `mut` is set once, when the record is created.
- Two fields of the same record cannot have the same name (E0207).

### `enum`

- An enum has one or more **cases**, names without payload.
- Two cases of the same enum cannot have the same name (E0207).

### `variant`

- A variant has one or more cases, each with an optional payload of **one** type: `percent(u8)`. To carry several values, the payload is a record.
- Two cases of the same variant cannot have the same name (E0207).

### Names

- The names of the types and functions of an interface form **one namespace**, as in WIT: a type and a function of the same interface cannot have the same name (E0206). Two functions with the same name remain E0202.
- A type cannot have the name of a top-level declaration of the package (a world, an interface or a global, E0210). This keeps every path unambiguous: in `a::b`, `a` is either a type of the current interface or an interface, never both.
- Field and case names live in their record, enum or variant. A field can have the name of a type (`status: mut status`).
- Every name follows the identifier rules of § 4.3, and converts to WIT by replacing `_` with `-` (§ 13.2): `unit_price` → `unit-price`, `invoice_error` → `invoice-error`.

## v2.2.4 Naming a type

| Where | How | Example |
|---|---|---|
| in the interface that declares it | its name | `line` |
| in another interface | the interface, then the name | `invoices::line` |
| in the type of a global | the interface, then the name | `global first: invoices::line = …;` |

There is only one level of path, as for function calls (§ 9.1). Using an unknown type, or naming a function, an interface or a world where a type is expected, is an error (E0305).

The built-in types are keywords and are written the same everywhere: `string`, `list<u8>`, `option<invoices::line>`.

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

- Every cycle of types must go through an `option` or a `list`, which can be empty; otherwise no value of the type can be built, and the declaration is an error (E0211): `record a { next: a }`.
- The Component Model does not allow recursive types. A recursive type can therefore only be used **inside** a component: using it, directly or through another type, in the signature of an exported function is an error (E0208).

## v2.2.6 The read-only view of a mutable list

`list<mut T>` can be used wherever `list<T>` is expected, with the same `T`: as an argument, a returned value, an assigned value, a field value or an initial value. Nothing is converted or copied: the receiver gets the same list, with read-only access.

```wasl
func digits() -> list<u8> {
  local d: list<mut u8> = list::filled(3, 0);
  d[0] = 1;
  return d;          // valid: list<mut u8> is a read-only list<u8>
}
```

- This is the only relation between distinct types in v0.2. It does not apply inside other types: `option<list<mut u8>>` is not an `option<list<u8>>`.
- `list<T>` means *read-only*, not *immutable*: the elements of a `list<u8>` can change if another part of the program holds the same list as a `list<mut u8>`. Use `list::copy` (§ v2.4.11) to get a list that nobody else can modify.
- The opposite direction does not exist: a `list<T>` is never a `list<mut T>` (E0400). `list::copy` gives a new `list<mut T>` with the same elements.

## v2.2.7 Types in signatures

Any type can be used for parameters, return values, locals, globals, fields, list elements and payloads. The restrictions only concern **exported** functions:

- no recursive type (E0208, § v2.2.5);
- every type they use must be declared in an interface that can appear in the WIT of the world (E0209, § v2.7.1).
