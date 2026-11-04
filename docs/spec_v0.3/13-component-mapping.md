# 13. Mapping to the Component Model

This chapter specifies the WIT of a package and how the core module of a world (chapter 12) becomes a component. The principles are in chapter 2.

## 13.1 Generated WIT

The **WIT of the package** contains, in this order:

1. the declaration `package <namespace>:<name>@<version>;` from the manifest (§ 3.2);
2. every interface that appears (below), in declaration order, with its types then its functions, each in declaration order;
3. every world, in declaration order, with its `import`s and `export`s in the order of the source.

Internal interfaces, globals and function bodies never appear (§ 2.6).

```wit
package example:calc@0.1.0;

interface math {
  fib: func(n: u32) -> u64;
  fib-rec: func(n: u32) -> u64;
  reset: func();
}

world calculator {
  export math;
}
```

- A function `func f(p: T, …) -> R` becomes `f: func(p: T, …) -> R;`; without a return type, `f: func(p: T, …);`.
- A world `world w { export i; }` becomes `world w { export i; }`. An import or an export of an interface of another package is written with its WIT name and the version of the dependency (§ 3.5): `import wasi:cli/stdout@0.2.6;`, `export wasi:cli/run@0.2.6;`. The foreign interfaces themselves are not written in the WIT of the package: they belong to their packages, and the component embeds them (below).
- A resource becomes `resource r { … }` with its constructor (`constructor(p: T, …);`), its methods (`m: func(…) -> R;`) and its static functions (`s: static func(…) -> R;`), in declaration order; its fields never appear (§ 7.12). A parameter `c: r` is written `c: r` (an `own`), `c: borrow<r>` as is.
- `weasel wit` prints the WIT of the package (§ 14.2). The layout of the printed text (indentation, blank lines) is not specified; its content is.
- The **component of a world** embeds the WIT of that world: the world, the interfaces it imports and exports, the package they belong to, and the packages of the dependencies they come from.
- The types of an interface appear in its WIT **before its functions**, in declaration order, with WIT names (§ 13.2): `unit_price` → `unit-price`.
- `mut` never appears: the WIT has no mutability, so the types it contains have no `mut` field and no `list<mut T>` (E0214, § 7.11).
- `string`, `list<T>`, `option<T>`, `result<…>`, `record`, `enum` and `variant` are written as in WIT, which uses the same names and the same forms.
- When an interface `a` uses a type of another interface `b` (in a function signature or in one of its types), the WIT of `a` starts with `use b.{t1, t2};`, listing the types of `b` it uses in declaration order, and the path is erased: `invoices::line` is written `line` in `a`. When `b` belongs to another package, the `use` names it with its package and version: `use example:catalog/carts@0.1.0.{cart, cart-error};`.

**Which interfaces appear.** An interface appears in the WIT of the package if a world exports it, or if an interface that appears uses one of its types. An interface of the second kind that no world exports is a **types-only interface** in WIT: the world that uses it imports it implicitly, as WIT requires, and the host has nothing to provide for it. This is only possible if it declares no function: an exported function that uses a type of an interface that has functions and is neither exported nor imported by the same world is an error (E0209). An interface of the package that a world imports also appears, with its functions: `import accounts;` needs the WIT of `accounts`.

**No cycle of `use`.** WIT forbids interfaces whose `use` form a cycle. Among the interfaces that appear, if `a` uses a type of `b` and `b`, directly or through other interfaces, uses a type of `a`, it is an error (E0213), reported on the first interface of the cycle in declaration order, with each step of the cycle. The fix is to move the shared types to an interface of their own. Interfaces that do not appear in the WIT can use each other's types freely.

```wasl
world shop {
  export orders;
}

interface types {
  record item {
    name: string,
    price: u64,
  }
}

interface orders {
  func cheapest(items: list<types::item>) -> option<types::item> { … }
}
```

```wit
package example:shop@0.1.0;

interface types {
  record item {
    name: string,
    price: u64,
  }
}

interface orders {
  use types.{item};

  cheapest: func(items: list<item>) -> option<item>;
}

world shop {
  export orders;
}
```

The WIT of the world, embedded in its component, contains `import types;` added by WIT elaboration; it is not written in the WIT of the package. Types used only by internal interfaces never appear.

## 13.2 Names

- A WaSL identifier becomes a WIT identifier by replacing each `_` with `-`: `fib_rec` → `fib-rec`, `circle_area` → `circle-area`, `get_calls` → `get-calls`. The identifier rules of § 4.3 guarantee that the result is a valid WIT identifier and that two different WaSL names never give the same WIT name.
- This applies to interfaces, worlds, functions, parameters, types, fields, enum cases, variant cases and flags: `invoice_error` → `invoice-error`, `discount_too_large` → `discount-too-large`.
- The primitive and built-in types have the same names in WaSL and in WIT (§ 7.1, § 7.6).
- A WaSL identifier may coincide with a WIT keyword that is not reserved in WaSL (for instance `from`). The WIT then writes it with the WIT escape `%` (`%from`), which is the standard way to use a keyword as a name in WIT.
- **Names of other packages** go the other way: a WIT name of a dependency is written in WaSL by replacing each `-` with `_` (`incoming-handler` → `incoming_handler`, `output-stream` → `output_stream`, `blocking-write-and-flush` → `blocking_write_and_flush`), and the namespace and package the same way (`my-org:http-tools` → `my_org:http_tools`). A WIT name that gives no valid WaSL identifier, because it has uppercase letters, a word that starts with a digit or is a WaSL keyword or reserved word (§ 4.4), cannot be named from WaSL: using the item is E0215, and the interface stays usable for its other items. The WIT escape `%` is removed: `%from` is `from`.

## 13.3 Exports of the core module

The core module (§ 12.1) exports the functions of the exported interfaces under the **standard Component Model names**:

```
cm32p2|<interface>|<function>
```

- `<function>` is the WIT name of the function (`fib-rec`).
- `<interface>` is the **canonical name** of the interface: `<namespace>:<name>/<interface>@<canonical version>`, with WIT names.
- The canonical version only keeps the part of the version that defines compatibility, following SemVer:

| Version | Canonical version |
|---|---|
| `1.2.3` | `1` |
| `0.2.1` | `0.2` |
| `0.0.1` | `0.0.1` |
| `1.0.0-rc1` | `1.0.0-rc1` (a pre-release is kept entirely) |
| `1.2.3+abc` | `1` (build metadata is dropped) |

Example: the function `fib_rec` of the interface `math` of the package `example:calc@0.1.0` is exported as `cm32p2|example:calc/math@0.1|fib-rec`.

## 13.4 Canonical ABI

A component describes its functions with component-level types; the Canonical ABI defines how those values are passed to and from the core functions.

- Scalars are passed directly as core values:

| WaSL / WIT type | Core type |
|---|---|
| `bool`, `s8`, `s16`, `s32`, `u8`, `u16`, `u32`, `char`, enum, flags | `i32` |
| `s64`, `u64` | `i64` |
| `f32` | `f32` |
| `f64` | `f64` |

- When the host passes a `bool`, `s8`, `u8`, `s16` or `u16`, the Canonical ABI already gives the core function a normalized `i32` (§ 12.2); when the core function returns one, the component reads the low-order bits.
- Strings, lists, and values that do not fit in the flat limits go through linear memory (§ 13.5). A function whose signature only has scalars is lifted without canonical options: no `memory`, no `realloc`, no `post-return`, no `string-encoding` (§ 13.6).

## 13.5 Copies through linear memory

The Canonical ABI passes component values to a core function as **flat** core values (an `i32` per enum, per string pointer, per string length…) and through **linear memory** for strings, lists, and anything that does not fit in the flat limits. WasmGC references cannot cross the boundary.

So every exported function of a world gets a generated **adapter**, which is the core function the component actually exports (§ 13.8):

1. it **lifts** the arguments: it reads the flat values and the bytes the host wrote in linear memory, and builds new WasmGC objects (a deep copy);
2. it calls the WaSL function with these objects;
3. it **lowers** the result: it writes the result into linear memory (a deep copy), and returns the flat values or a pointer to them.

Functions called from inside the component never go through an adapter (§ 10.10).

WebAssembly has no instruction that copies between a GC array and linear memory, so strings and lists of numbers are copied element by element (`i32.load8_u` then `array.set` when lifting, `array.get_u` then `i32.store8` when lowering). The cost of a boundary crossing is proportional to the size of the values that cross it, which the `gc` option of the Canonical ABI will remove (§ 16).

## 13.6 Canonical options

An exported function is lifted with these canonical options **only when its signature needs linear memory**, that is when, following the flattening rules of the Canonical ABI, its parameters contain a string or a list or flatten to more than 16 values, or its result contains a string or a list or flattens to more than 1 value:

| Option | Value |
|---|---|
| `memory` | the memory of the core module, exported as `cm32p2_memory` |
| `realloc` | the allocator of § 13.7, exported as `cm32p2_realloc` |
| `string-encoding` | `utf8`, the encoding of WaSL strings |
| `post-return` | the function that releases the result (§ 13.7), exported as `cm32p2|<interface>|<function>_post`, only for a function whose result goes through memory |

A function whose result is returned as flat values (a scalar, or no result) has no post-return, even if its parameters go through memory: `func f(s: string) -> u32` needs the memory and `realloc`, but leaves nothing to release. The module has a memory and a `realloc` only if at least one of its exported functions needs them: a world whose exported functions only use scalars has no memory (§ 13.4).

Because parameters beyond 16 flat values go through memory, exported functions have **no parameter limit** (§ 6.5).

## 13.7 Linear memory as a scratch buffer

Linear memory only holds values **during** a boundary crossing: never a WaSL value, never between two calls. It is managed by a bump allocator, with one mutable `i32` global, `$scratch_top`, starting at 0:

- **`cm32p2_realloc(old_ptr, old_size, align, new_size) -> ptr`**: rounds `$scratch_top` up to `align`, returns it as `ptr` and adds `new_size`; if the memory is too small, it grows it with `memory.grow`, and traps if the runtime refuses. If `old_ptr` is not 0, the `old_size` bytes at `old_ptr` are copied to `ptr`. Nothing is ever freed individually.
- After the adapter has lifted the arguments, it sets `$scratch_top` back to 0: the arguments are now on the WasmGC heap.
- Lowering the result allocates from 0 again. The **post-return** function sets `$scratch_top` back to 0 once the host has read the result.

The memory starts with zero pages and grows to the size of the largest single crossing; it never shrinks. This is enough because a component is never re-entered: a synchronous export cannot be called again while it runs, and an import cannot call the component back. The calls to imports use the same buffer, in the reverse direction (§ 13.10). Async will extend this scheme.

## 13.8 Adapters

For an exported function `f` of the interface `i`, the core export `cm32p2|<i>|<f>` (§ 13.3) is the adapter; the WaSL function itself is an internal function of the module. When `f` only has scalar parameters and a scalar or no result, the WaSL function is exported directly.

**Lifting** a value of type `T` from its Canonical ABI representation (flat values, or bytes at an address, laid out as the Canonical ABI specifies):

| Type | Lifting |
|---|---|
| primitive types, `char`, enum, flags | the value itself (§ 13.4); for flags, the bit set; the Canonical ABI guarantees that a `char` is a Unicode scalar value |
| `string` | `(ptr, len)`: a new `$bytes` of `len` bytes, copied from `ptr`. The Canonical ABI guarantees valid UTF-8. |
| `list<T>` | `(ptr, len)`: a new `$list_T` whose element `k` is lifted from `ptr + k × size(T)` |
| record, tuple | each field or element lifted in order, then `struct.new` |
| variant, `result` | the discriminant selects the case; its payload is lifted, then the case is created (§ 12.6) |
| `option<T>` | discriminant 0: `ref.null`; 1: the payload, lifted and boxed if needed (§ 12.7) |

**Lowering** is the reverse: strings and lists are written to memory allocated with `cm32p2_realloc`, and the other types are written in place, following the layout of the Canonical ABI. When the result flattens to more than one value, it is written to a return area, also allocated with `cm32p2_realloc`, and its address is returned.

The adapters are generated per type and shared by every exported function that uses the same type; a list of records, for instance, uses the lifting function of the record in a loop.

## 13.9 Building the component

The reference implementation builds the component in three steps:

1. it generates the core module of the world (chapter 12), with WasmGC types and, when needed, a memory and the exports of § 13.6;
2. it attaches the WIT of the world to the module, as a `component-type` custom section (`wit_component::embed_component_metadata`);
3. it wraps the module into a component with `wit_component::ComponentEncoder`, which reads that section, checks that every exported function exists with the right core signature, finds the memory, `realloc` and post-return functions by their standard names, generates the lifts with their canonical options and validates the result.

The result is a standard component:

```sh
wasm-tools validate build/calculator.wasm
wasm-tools component wit build/calculator.wasm     # prints the WIT of the world
wasm-tools print build/calculator.wasm             # prints the component as text
```

## 13.10 Imports

Each function of an imported interface that the component calls is a **core import** of the module, lowered by the Component Model with `canon lower`, under the standard names:

```
module: cm32p2|<interface>         name: <function>
```

- `<interface>` is the canonical name of § 13.3: `cm32p2|wasi:cli/stdout@0.2` for `wasi:cli/stdout@0.2.6`, `cm32p2|example:catalog/carts@0.1` for the `carts` of `example:catalog@0.1.0`. An interface of the same package (§ 2.5) has the name of the package.
- `<function>` is the WIT name of the function: `get-stdout`; for a resource, the names of the Component Model: `[constructor]cart`, `[method]cart.add`, `[static]cart.of`.
- Each imported resource that the component uses (specified, not available yet) adds the import `<resource>_drop` (`output-stream_drop`), the `resource.drop` of § 12.10, of type `(func (param i32))`.

**Reverse adapters.** A call to an imported function goes through a generated function that does the work of § 13.8 in the reverse direction: it **lowers** the arguments (strings and lists written to linear memory allocated with `cm32p2_realloc`), calls the import, then **lifts** the result into new WasmGC objects, and sets `$scratch_top` back to where it was before the call. Arguments that flatten to more than 16 values are written to memory and passed by address; when the result flattens to more than one value, the caller allocates the return area and passes its address as the last argument. An imported function whose parameters and result are all scalars is called directly, without an adapter. A module whose imported functions need memory has a memory and a `realloc`, even if its exported functions do not. Canonical options for imports follow § 13.6: `memory`, `realloc` and `utf8` when the signature needs them, never a post-return.

**Handles** (specified, not available yet). An own handle passed as an argument is given to the callee: the Component Model removes it from the handle table, which is why WaSL sets the local to `0` (§ 9.9). A borrowed handle is passed as is and stays in the table. A handle received in a result is a new entry of the table, owned by the place that receives it.

**Same package.** For `import accounts;` the component imports the functions of `accounts` that it calls, under the name of the package; the functions of `accounts` are not compiled into it (§ 2.5). The component of the world that exports `accounts` provides them when the two are composed (§ 14.2).

## 13.11 Resources at the boundary

A resource `r` of an exported interface `i` is a WasmGC struct inside the component (§ 12.10), but the Component Model gives other components an `i32` handle whose **representation** (`rep`) is chosen by the component. A WasmGC reference cannot be a `rep`, so the module keeps a **rep table**:

- a mutable global `$reps_r`, a growable `(array (mut (ref null $r)))`, and a stack of its empty slots, `$frees_r`, an `(array (mut i32))` as large as `$reps_r`, with the count of its entries;
- `rep` is an index in this array. Each handle given out gets its own slot, which holds the object, and keeps it alive, until the handle is released. A new handle takes the last freed slot, or else the next unused one; when the array is full, both arrays are replaced by arrays twice as large (8 slots at first), and the objects are copied with `array.copy`.

The module imports the intrinsics of the Component Model for `r`, under the module `cm32p2|_ex_<interface>`, and exports its destructor:

| Name | Type | Role |
|---|---|---|
| `r_new` (import) | `(func (param i32) (result i32))` | `resource.new`: creates a handle for a `rep` |
| `r_rep` (import) | `(func (param i32) (result i32))` | `resource.rep`: the `rep` of a handle |
| `r_drop` (import) | `(func (param i32))` | `resource.drop`: releases a handle owned by the component |
| `cm32p2\|<interface>\|r_dtor` (export) | `(func (param i32))` | called by the Component Model when the handle is released: empties the slot `rep` |

**Lowering** an object (a result, or the argument of an import) stores it in a free slot and returns `r_new(slot)`. **Lifting** a handle: for an `own<r>`, `r_rep` gives the slot, the object is read from it, then `r_drop` releases the handle, which runs the destructor and empties the slot: the object is now an ordinary WasmGC value of the component; for a `borrow<r>` of the component's own resource, the Component Model passes the `rep` directly, and the object is read from the slot.

The constructor, the methods and the static functions are exported like functions of the interface (§ 13.3), with the names of the Component Model: `cm32p2|example:catalog/carts@0.1|[constructor]cart`, `…|[method]cart.add`, `…|[static]cart.of`. Their adapters lift `self` as a `borrow<r>`.

## 13.12 Foreign exports (specified, not available yet)

The functions of the block `export wasi:cli/run { … }` (§ 2.4.2) are exported like the functions of an interface of the package: with the canonical name of the foreign interface, `cm32p2|wasi:cli/run@0.2|run`, their adapters (§ 13.8) and their canonical options (§ 13.6).

The compiler reads the WIT of the foreign interface from the dependency (§ 3.5) and checks the block against it (E0218): every function of the interface, with the same parameter names and types, the same result, and no other function. The types are compared after the conversion of names of § 13.2. A foreign interface that declares resources cannot be exported in 0.3: the block would have to implement them, which needs a syntax for resources in foreign exports (§ 16).
