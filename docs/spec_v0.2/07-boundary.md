# v2.7 The component boundary

This chapter extends chapter 13: the WIT of the new types, and how their values cross the boundary of a component. Inside a component, values live on the WasmGC heap (chapter v2.6). The Canonical ABI, however, only knows linear memory today: values that cross the boundary are copied through it, in both directions.

## v2.7.1 Generated WIT

The derivation by erasure of § 2.6 and § 13.1 is extended:

- The types of an interface appear in its WIT **before its functions**, in declaration order, with WIT names (§ 13.2): `unit_price` → `unit-price`.
- `mut` is erased: a `mut` field is a plain field, and `list<mut T>` is `list<T>`.
- `string`, `list<T>`, `option<T>`, `result<…>`, `record`, `enum` and `variant` are written as in WIT, which uses the same names and the same forms.
- When an interface `a` uses a type of another interface `b` (in a function signature or in one of its types), the WIT of `a` starts with `use b.{t1, t2};`, listing the types of `b` it uses in declaration order, and the path is erased: `invoices::line` is written `line` in `a`.

**Which interfaces appear.** An interface appears in the WIT of the package if a world exports it (§ 13.1), or if an interface that appears uses one of its types. An interface of the second kind that no world exports is a **types-only interface** in WIT: the world that uses it imports it implicitly, as WIT requires, and the host has nothing to provide for it. This is only possible if it declares no function: an exported function that uses a type of an interface that has functions and is not exported by the same world is an error (E0209).

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

## v2.7.2 Copies through linear memory

The Canonical ABI passes component values to a core function as **flat** core values (an `i32` per enum, per string pointer, per string length…) and through **linear memory** for strings, lists, and anything that does not fit in the flat limits. WasmGC references cannot cross the boundary.

So every exported function of a world gets a generated **adapter**, which is the core function the component actually exports (§ v2.7.5):

1. it **lifts** the arguments: it reads the flat values and the bytes the host wrote in linear memory, and builds new WasmGC objects (a deep copy);
2. it calls the WaSL function with these objects;
3. it **lowers** the result: it writes the result into linear memory (a deep copy), and returns the flat values or a pointer to them.

Functions called from inside the component never go through an adapter (§ v2.5.4).

WebAssembly has no instruction that copies between a GC array and linear memory, so strings and lists of numbers are copied element by element (`i32.load8_u` then `array.set` when lifting, `array.get_u` then `i32.store8` when lowering). The cost of a boundary crossing is proportional to the size of the values that cross it, which the `gc` option of the Canonical ABI will remove (§ v2.11).

## v2.7.3 Canonical options

An exported function is lifted with these canonical options **only when its signature needs linear memory**, that is when, following the flattening rules of the Canonical ABI, its parameters contain a string or a list or flatten to more than 16 values, or its result contains a string or a list or flattens to more than 1 value:

| Option | Value |
|---|---|
| `memory` | the memory of the core module, exported as `cm32p2_memory` |
| `realloc` | the allocator of § v2.7.4, exported as `cm32p2_realloc` |
| `string-encoding` | `utf8`, the encoding of WaSL strings |
| `post-return` | the function that releases the result (§ v2.7.4), exported as `cm32p2|<interface>|<function>_post` |

The module has a memory and a `realloc` only if at least one of its exported functions needs them: a world whose exported functions only use scalars is built exactly as in v0.1 (§ 13.4), with no memory.

Because parameters beyond 16 flat values now go through memory, exported functions have **no parameter limit**: E0205 (§ 6.5) is no longer reported.

## v2.7.4 Linear memory as a scratch buffer

Linear memory only holds values **during** a boundary crossing: never a WaSL value, never between two calls. It is managed by a bump allocator, with one mutable `i32` global, `$scratch_top`, starting at 0:

- **`cm32p2_realloc(old_ptr, old_size, align, new_size) -> ptr`**: rounds `$scratch_top` up to `align`, returns it as `ptr` and adds `new_size`; if the memory is too small, it grows it with `memory.grow`, and traps if the runtime refuses. If `old_ptr` is not 0, the `old_size` bytes at `old_ptr` are copied to `ptr`. Nothing is ever freed individually.
- After the adapter has lifted the arguments, it sets `$scratch_top` back to 0: the arguments are now on the WasmGC heap.
- Lowering the result allocates from 0 again. The **post-return** function sets `$scratch_top` back to 0 once the host has read the result.

The memory starts with zero pages and grows to the size of the largest single crossing; it never shrinks. This is enough because a v0.2 component is never re-entered: a synchronous export cannot be called again while it runs. Imports and async (later versions) will extend this scheme.

## v2.7.5 Adapters

For an exported function `f` of the interface `i`, the core export `cm32p2|<i>|<f>` (§ 13.3) is the adapter; the WaSL function itself is an internal function of the module. When `f` only has scalar parameters and a scalar or no result, the WaSL function is exported directly, as in v0.1.

**Lifting** a value of type `T` from its Canonical ABI representation (flat values, or bytes at an address, laid out as the Canonical ABI specifies):

| Type | Lifting |
|---|---|
| v0.1 scalars, `char`, enum, flags | the value itself (§ 13.4); for flags, the bit set; the Canonical ABI guarantees that a `char` is a Unicode scalar value |
| `string` | `(ptr, len)`: a new `$bytes` of `len` bytes, copied from `ptr`. The Canonical ABI guarantees valid UTF-8. |
| `list<T>` | `(ptr, len)`: a new `$list_T` whose element `k` is lifted from `ptr + k × size(T)` |
| record, tuple | each field or element lifted in order, then `struct.new` |
| variant, `result` | the discriminant selects the case; its payload is lifted, then the case is created (§ v2.6.2) |
| `option<T>` | discriminant 0: `ref.null`; 1: the payload, lifted and boxed if needed (§ v2.6.3) |

**Lowering** is the reverse: strings and lists are written to memory allocated with `cm32p2_realloc`, and the other types are written in place, following the layout of the Canonical ABI. When the result flattens to more than one value, it is written to a return area, also allocated with `cm32p2_realloc`, and its address is returned.

The adapters are generated per type and shared by every exported function that uses the same type; a list of records, for instance, uses the lifting function of the record in a loop.

## v2.7.6 Names

The rules of § 13.2 extend to the new names: types, fields, enum cases and variant cases convert from `snake_case` to `kebab-case` (`invoice_error` → `invoice-error`, `discount_too_large` → `discount-too-large`), with the `%` escape for WIT keywords (§ 13.2).

## v2.7.7 Building the component

The three steps of § 13.5 are unchanged: the module now contains WasmGC types and, when needed, a memory and the exports of § v2.7.3, which `wit_component::ComponentEncoder` finds by their standard names and turns into the canonical options of each lifted function.
