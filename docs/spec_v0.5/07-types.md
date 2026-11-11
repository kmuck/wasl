# 7. Types

## 7.1 Primitive types

WaSL has eleven primitive types. They are exactly the primitive types of WIT with the same names, and each one is represented by a core Wasm value type. The other types are in § 7.5.

| Type | Values | Wasm representation |
|---|---|---|
| `bool` | `true`, `false` | `i32` (0 or 1) |
| `s8` | −128 … 127 | `i32` |
| `s16` | −32,768 … 32,767 | `i32` |
| `s32` | −2³¹ … 2³¹−1 | `i32` |
| `s64` | −2⁶³ … 2⁶³−1 | `i64` |
| `u8` | 0 … 255 | `i32` |
| `u16` | 0 … 65,535 | `i32` |
| `u32` | 0 … 2³²−1 | `i32` |
| `u64` | 0 … 2⁶⁴−1 | `i64` |
| `f32` | IEEE 754 binary32 (single precision) | `f32` |
| `f64` | IEEE 754 binary64 (double precision) | `f64` |

The representation of 8- and 16-bit integers inside an `i32` is specified in § 12.2.

## 7.2 Categories

The typing rules refer to these categories:

- **signed integers**: `s8`, `s16`, `s32`, `s64`;
- **unsigned integers**: `u8`, `u16`, `u32`, `u64`;
- **integers**: signed and unsigned integers;
- **floats**: `f32`, `f64`;
- **numeric types**: integers and floats;
- **scalar types**: the primitive types, `char`, enums and flags. They are values, represented by a Wasm number;
- **heap types**: every other type (§ 7.5). Their values are references to objects of the WasmGC heap (§ 10.8). A resource is a heap type, even where it is represented by a handle (§ 7.12);
- **case types**: enums, variants, options and results, the types a `match` inspects (§ 8.16).

`bool` is in none of the numeric categories.

## 7.3 Distinct types

All types are distinct. No type converts implicitly to another: a `u32` cannot be used where a `u64` is expected, an integer cannot be used as a condition, a `bool` is not a number. Two records with the same fields are two different types, and `list<T>` and `list<mut T>` are two different types. Conversions between numeric types are written explicitly (§ 8.5). A type alias is not a new type: it is a second name of the type it aliases (§ 7.14).

## 7.4 Functions without a value

A function without a return type returns no value. A call to such a function is not an expression of any type: it can only be used as a statement (§ 9.6). There is no `void` or unit type that can be written; `_` in `result<_, E>` only marks a case without payload (§ 7.6).

## 7.5 Heap and declared types

Eleven kinds of types complete the primitive types of § 7.1. They are exactly the WIT types of the same name.

| Type | Values | Declared | WasmGC representation (chapter 12) |
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
| `resource c { … }` | an object with a private state and methods | in an interface | `(ref $c)`, a struct; an `i32` handle when it belongs to another component (§ 7.12) |

The categories of § 7.2 apply: `char`, enums and flags are scalar types, the others are heap types.

## 7.6 Built-in types

### `char`

- A `char` is a Unicode scalar value: a code point from `U+0000` to `U+10FFFF`, surrogates (`U+D800` to `U+DFFF`) excluded. Every `char` value is valid.
- It is written as a literal, `'a'`, `'é'`, `'\n'`, `'\u{1F600}'` (§ 4.9), compared with `==`, `!=`, `<`, `<=`, `>`, `>=` in code point order (§ 8.3), and converted to its code point with `u32(c)` (§ 8.5).
- It is a scalar: a local of type `char` without initial value is `'\u{0}'`.

### `string`

- A string is an immutable sequence of Unicode scalar values, encoded in UTF-8. Its length (`string.len`) is its number of **bytes**.
- A string is always valid UTF-8: every way to build one (literals, `+`, `string.from_bytes`, `string.from_chars`, the component boundary) guarantees it.
- Strings are compared by content with `==` and `!=`, and concatenated with `+` (§ 8.3).

### `list<T>` and `list<mut T>`

- A list has a **fixed length**, chosen when it is created, like a WebAssembly array: it never grows nor shrinks (§ 12.5).
- A `list<T>` is **immutable**: once it is created, none of its elements ever changes, whoever holds it. A `list<mut T>` also allows `xs[i] = e;`. The two are distinct types (§ 7.10).
- `T` is any type, including another list: `list<list<u8>>`. `mut` only applies to the elements of the list it is written on: in `list<mut list<u8>>` the outer list is mutable, the inner ones are not.

### `tuple<T1, …, Tn>`

- A tuple groups one or more values of fixed types, without names: `tuple<u32, u64>`.
- It is created with `tuple(e1, …, en)` and its elements are read with `t.0`, `t.1`… (§ 8.13). Its elements are immutable, as in a record without `mut` field.

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

## 7.7 Declared types

`record`, `enum`, `variant`, `flags` and `resource` types are declared **inside an interface**, next to its functions, exactly as in WIT.

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
    status: status,
  }

  func total(inv: invoice) -> result<u64, invoice_error> { … }

}
```

Resources have their own section (§ 7.12). In the other kinds of declaration, the fields, cases or flags are separated by commas; a trailing comma is allowed. There is at least one, and two of them cannot have the same name (E0207).

### `record`

- A record has one or more **fields**, each with a name and a type.
- A field declared `mut` can be assigned (`c.hits = c.hits + 1;`); a field without `mut` is set once, when the record is created. This is exactly the mutability of WasmGC struct fields (§ 12.5).
- A record with a `mut` field stays inside the component: it cannot appear in the WIT (E0214, § 7.11).

### `enum`

An enum has one or more **cases**, names without payload.

### `variant`

A variant has one or more cases, each with an optional payload of **one** type: `percent(u8)`. To carry several values, the payload is a record or a tuple.

### `flags`

- A flags type has one to 32 **flags** (E0212 beyond, the limit of the Component Model). A value is a set of these flags: any combination, including none or all of them.
- It is written as a literal listing the flags it contains, `delivery { email, urgent }`, or `delivery { }` for the empty set; `f.paper` tells whether a flag is in the set; `|` and `&` give the union and the intersection (§ 8.3).
- Flags values are scalars, like integers: they have no identity and are copied, never shared. A local of a flags type without initial value is the empty set.

### Names

- The names of the types and functions of an interface form **one namespace**, as in WIT: a type and a function of the same interface cannot have the same name (E0206). Two functions with the same name remain E0202.
- A type cannot have the name of a top-level declaration of the package (a world, an interface or a global, E0210). This keeps every path unambiguous: in `a::b`, `a` is either a type of the current interface or an interface, never both.
- Field, case and flag names live in their type. A field can have the name of a type (`status: status`).
- Every name follows the identifier rules of § 4.3, and converts to WIT by replacing `_` with `-` (§ 13.2): `unit_price` → `unit-price`, `invoice_error` → `invoice-error`.

## 7.8 Naming a type

| Where | How | Example |
|---|---|---|
| in the interface that declares it | its name | `line` |
| in another interface | the interface, then the name | `invoices::line` |
| in the type of a global | the interface, then the name | `global first: invoices::line = …;` |

There is only one level of path, as for function calls (§ 9.1). Using an unknown type, or naming a function, an interface or a world where a type is expected, is an error (E0305).

The built-in types are keywords and are written the same everywhere: `string`, `list<u8>`, `option<invoices::line>`, `tuple<char, u32>`.

## 7.9 Recursive types

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

## 7.10 Immutable and mutable lists

`list<T>` and `list<mut T>` are two distinct types. Neither can be used where the other is expected (E0400); going from one to the other always creates a new list:

| From | To | With |
|---|---|---|
| `list<mut T>` | `list<T>` | `list.freeze(xs)`: a new immutable list with the elements of `xs` |
| `list<T>` or `list<mut T>` | `list<mut T>` | `list.copy(xs)`: a new mutable list with the elements of `xs` |

```wasl
func digits() -> list<u8> {
  local d: list<mut u8> = list.filled(3, 0);
  d[0] = 1;
  return list.freeze(d);    // `return d;` would be E0400
}
```

Why immutability is a guarantee and not a read-only view:

- **A `list<T>` can be trusted.** A function that receives or stores a `list<T>` knows that its elements will never change, whatever the rest of the program does. Only `list<mut T>` values can be shared and modified at the same time.
- **It prepares copy-free boundaries.** With the `gc` option of the Canonical ABI (§ 16), an immutable list can be handed to another component by reference, since nobody can modify it. A mutable list never crosses the boundary (§ 7.11). The language must therefore know, for every list, that it is really immutable.

Most lists are built once, with a literal, `list.filled` or at the boundary, and need no `freeze`. The copy made by `list.freeze` is the price of building an immutable list element by element; the representation that makes it necessary today is explained in § 12.5.

## 7.11 Types in the WIT

Any type can be used for parameters, return values, locals, globals, fields, list and tuple elements and payloads. The restrictions only concern what appears in the WIT of a world: the signatures of **exported** functions, and every type declared in an **exported interface**, which appears in the WIT even if no function uses it (§ 13.1):

- no recursive type (E0208, § 7.9);
- every type they use must be declared in an interface that can appear in the WIT of the world (E0209, § 13.1);
- nothing that can change: no `mut` field and no `list<mut T>`, at any depth (E0214);
- no writable end of a stream or a future, `stream_writer<T>` or `future_writer<T>`, at any depth (E0222, § 7.13): WIT only names readable ends.

**Mutability stays inside the component.** The WIT has no mutability, as it has no recursive types: what it cannot express stays internal. A record with a `mut` field, or a type that contains a `list<mut T>`, is declared in an interface that no world exports, and does not appear in an exported signature. An exported function that needs to change a list it receives takes a `list<T>` and works on `list.copy(xs)`; one that changes a record returns a new record.

This makes exported functions behave the same for every caller. Values that cross the boundary are copied today, while calls inside the component share them (§ 10.10). Since nothing that crosses can change, and the language has no identity comparison, a copy and a shared reference cannot be told apart. The same holds when the `gc` option of the Canonical ABI lets references cross without copy (§ 16).

```wasl
interface counters {          // exported by a world
  record counter { hits: mut u32 }       // E0214: `hits` is `mut`
  func sort(xs: list<mut u32>) { … }     // E0214: `list<mut u32>`
  func sorted(xs: list<u32>) -> list<u32> {
    local ys: list<mut u32> = list.copy(xs);
    // … sort ys in place …
    return list.freeze(ys);
  }
}
```

## 7.12 Resources

A resource is the object of the Component Model: a type with a private state and **methods**. Other components never see its state; they hold a **handle** to it and call its methods. A resource is declared in an interface, like the other types:

```wasl
interface carts {

  resource cart {
    items: mut u32,
    amount: mut u64,

    constructor() {
      return cart { items: 0, amount: 0 };
    }

    func add(price: u64, quantity: u32) {
      self.items = self.items + quantity;
      self.amount = self.amount + u64(quantity) * price;
    }

    func total() -> u64 {
      return self.amount;
    }

    static func single(price: u64) -> cart {
      local c: cart = cart();
      c.add(price, 1);
      return c;
    }
  }

}
```

```wit
interface carts {
  resource cart {
    constructor();
    add: func(price: u64, quantity: u32);
    total: func() -> u64;
    single: static func(price: u64) -> cart;
  }
}
```

### State

- The **fields** of a resource are its private state. They are declared like the fields of a record, each followed by `,`, and may be `mut`. A resource may have no field.
- The fields never appear in the WIT, which only has the constructor, the methods and the static functions. A `mut` field is therefore allowed in a resource of an exported interface: E0214 does not apply to it (§ 7.11).
- The fields can only be named inside the interface that declares the resource (E0309): `self.items`, `c.items` for any `c` of type `cart`, and the **resource literal** `cart { items: 0, amount: 0 }`, written like a record literal (§ 8.11), which creates a new object. A resource without field is written `cart {}`. Outside the interface, a resource is only reached through its constructor, its methods and its static functions.

### Constructor, methods, static functions

- **Constructor.** `constructor(params) { … }` declares at most one constructor (E0202). Its body returns the new object, `return cart { … };`, like a function whose return type is the resource. It is called with the name of the resource, like a conversion: `cart()` in its interface, `carts::cart()` elsewhere (§ 8.20). A resource without constructor can only be created by the functions of its interface.
- **Methods.** `func name(params) -> R { … }` inside the resource declares a method. Its body names the object it is called on `self`; `self` itself cannot be assigned (§ 5.3), but its `mut` fields can. A method is called on a value: `c.add(150, 2)`.
- **Static functions.** `static func name(params) -> R { … }` declares a function without `self`, called with the path of the resource: `cart::single(150)` in its interface, `carts::cart::single(150)` elsewhere.
- The constructor, the methods and the static functions of a resource, and its fields, form one namespace (E0202). Their names follow § 4.3 and convert to WIT as every name (§ 13.2).

### Resources of the package and handles

A resource has two representations, depending on where its object lives (§ 12.10, § 13.11):

- A resource of an interface of the package that the world does **not** import is a WasmGC struct inside the component, exactly like a record whose fields are private: values are references, shared and reclaimed by the runtime like any heap value (§ 10.8). When it crosses the boundary, the component gives the other side a handle to it, and gets it back when the other side gives the handle back.
- A resource of an interface of another package, or of an interface of the package that the world imports, lives in another component. The component only holds **handles** to it: `i32` indexes into the handle table that the Component Model keeps for the instance. A handle must be given back to its owner when it is no longer needed; WaSL does it automatically (§ 9.9).

A **handle type** is a resource type declared in an interface of another package, or in an interface of the package that at least one world imports. The ends of streams and futures, `stream<T>`, `stream_writer<T>`, `future<T>` and `future_writer<T>`, are handle types too (§ 7.13). The rules below apply to handle types everywhere in the package, so that the same code is valid in every world, whatever the representation the world gives it; the only exception is that the locals and parameters of the functions of the interface that declares the resource, which are only compiled where it is a struct, may hold it in any type:

- A handle type can be the type of a local, a parameter, a match binding, the return type of a function, and the payload of an `option`, a `result` or a variant, or an element of a tuple, that is returned or passed on at once.
- A value whose type contains a handle type, other than the handle type itself, is a **temporary**: the result of a call, or a constructor such as `option.some(c)` or `tuple(c, n)`, used at once by `?`, `match`, `return` or a destructuring `local (…) = e;`, or discarded by a call statement (§ 9.4, § 9.6, § 9.9).
- A handle type cannot appear in a record, a list, a global or a field, and a local, a parameter or a match binding cannot have a type that contains one without being one (E0220). This keeps the ownership of every handle in a single local, so that releasing it is a local decision (§ 9.9).
- A tuple that holds a handle is the way WASI 0.3 returns several handles at once: `types::response::new(…)` returns `tuple<response, future<…>>`, `stream.new()` returns both ends of a stream. Its destructuring gives each handle its own local (§ 9.4). A tuple was a forbidden place for a handle in 0.3. Reading one element of such a tuple, `types::response::new(…).0`, is E0220: it would lose the other handles.
- A local of a resource type needs an initial value (E0410), as for every heap type.
- Resources cannot be compared: `==` does not apply to them (§ 8.3).

### `borrow<T>`

- A parameter of type `T`, where `T` is a resource, **owns** its value: for a handle type, the caller gives the handle to the callee (§ 9.9). This is WIT's `own<T>`, which WaSL does not write.
- A parameter of type `borrow<T>` **borrows** it: the callee can call its methods during the call, but cannot keep it, return it or give it away. For a handle type, the caller keeps the handle.
- `borrow<T>` is only valid as the type of a parameter, with a resource `T` (E0219): `borrow<u32>`, a `borrow` return type or a `borrow` field are errors.
- `self` is a `borrow<T>` of its resource, as in WIT, where a method takes `self: borrow<T>`.

## 7.13 Streams and futures

A **stream** carries a sequence of values from one component to another, and a **future** carries one value that will be available later. They are the `stream<T>` and `future<T>` of WIT, with which WASI 0.3 passes the body of an HTTP request or the standard output of a command. Each of them has two ends:

| Type | Designates | In WIT |
|---|---|---|
| `stream<T>` | the readable end of a stream of `T` | `stream<T>` |
| `stream_writer<T>` | the writable end of a stream of `T` | none |
| `future<T>` | the readable end of a future of `T` | `future<T>` |
| `future_writer<T>` | the writable end of a future of `T` | none |

```wasl
local (body: stream<u8>, body_tx: stream_writer<u8>) = stream.new();
// `body` is given to another component, which reads it; this one writes to `body_tx`.
```

- **WIT only names readable ends.** A function that takes a `stream<T>` reads from it; a function that returns one gives its caller something to read. The writable end stays with the component that created the stream, which writes to it. So `stream_writer<T>` and `future_writer<T>` never appear in the WIT (E0222, § 7.11).
- **The four types are handle types** (§ 7.12): an `i32` index into the table of the instance, with one owner, moved and released automatically (§ 9.9). Releasing a readable end tells the writer that nobody reads any more. Releasing a `stream_writer<T>` closes the stream. Releasing a `future_writer<T>` before it has written its value traps, as the Component Model requires (§ 10.7).
- There is no `borrow` of an end: `borrow<T>` only applies to resources (E0219).
- They are created and used with the built-in functions of § 8.21. They cannot be compared (§ 8.3).
- **Elements.** The elements of a `stream<T>` cannot contain a handle type (E0220): they are written and read as lists (§ 8.21), and lists cannot hold handles. The value of a `future<T>` can: `future<result<option<types::trailers>, types::error_code>>` carries the handle of the trailers of an HTTP response, and reading it gives a temporary (§ 7.12).
- `stream` and `future` without an element type, which WIT allows, are not available (E0215, § 2.5).

**The types come from the WIT of the world.** The Component Model creates the two ends with an instruction that names the type, `stream.new` or `future.new`. The tools that build a component from a core module, which `waslc` uses (§ 13.9), only know the stream and future types that appear in the functions of the WIT of the world. So a function reachable from the exports of a world can only use a stream or a future whose readable type appears in a function of that world, imported or exported (E0223). Programs meet this rule naturally: a stream connects two components, and the function of the other side, which receives or returns the readable end, names its type. `stdout::write_via_stream(data: stream<u8>)` lets a component create a `stream<u8>`; `types::response::new(…)` lets it create the `stream<u8>` of a body and the `future<result<option<types::trailers>, types::error_code>>` of its trailers. A stream between two functions of the same component could not be used anyway: in 0.4, two async tasks never run at the same time in one instance (§ 13.13), and the Component Model only copies numbers between two ends held by the same instance.

## 7.14 Type aliases

*Specified, not available yet.*

A **type alias** gives a name to a type, inside an interface, as in WIT:

```wasl
interface shop {
  type price = u64;                          // cents
  type lines = list<order_line>;

  record order_line { sku: string, quantity: u32, unit_price: price }

  func total(lines: lines) -> price { … }
}
```

```wit
interface shop {
  type price = u64;
  type lines = list<order-line>;
  record order-line { sku: string, quantity: u32, unit-price: price }
  total: func(lines: lines) -> price;
}
```

- An alias is **the same type** as the type it names: a `price` is a `u64`, a `u64` is a `price`, without conversion. It exists to document, and to write a long type once.
- It is declared like the other types (§ 7.7): in an interface, in the namespace of its types and functions (E0206), named from another interface with a path, `shop::price` (§ 7.8). It can be used before its declaration.
- The aliased type can be any type, including a built-in type, a declared type, another alias or a type of another package. An alias cannot name itself, directly or through other aliases (E0225): `type a = list<a>;` is not a recursive type, it is an error.
- In the WIT, the alias is written `type name = T;` in its interface when the interface appears in the WIT (§ 13.1), and the signatures keep its name. An alias of a type that cannot appear in the WIT follows the rules of § 7.11 at the places where it is used.
- An alias declared in another package, such as `wasi:clocks/types.instant`, is named the same way (§ 2.5).

`type` only declares aliases. The other declarations keep their own word: `record`, `enum`, `variant`, `flags`, `resource`.
