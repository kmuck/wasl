# v2.5 Runtime semantics

This chapter extends chapter 10.

## v2.5.1 References and sharing

A value of a heap type is a **reference** to an object of the WasmGC heap. Inside a component:

- assigning it, passing it as an argument, returning it, storing it in a field, a list element or a payload copies the reference, never the object;
- two names that hold the same reference see the same object: a change made through one is visible through the other.

```wasl
local a: invoice = sample();
local b: invoice = a;
b.status = status::paid;      // a.status is now paid as well: a and b are the same record
```

Sharing is only observable through what can change: `mut` fields and the elements of `list<mut T>`. Strings, tuples and `list<T>` never change, so whether two of them are the same object cannot be observed; the language has no identity comparison (§ v2.11).

`list::filled(n, x)` shares `x` between all the elements: with a record `x` that has `mut` fields, changing the field through one element changes it for all of them.

**Lifetime.** An object lives as long as it is reachable. The runtime's garbage collector reclaims it afterwards; a WaSL program never frees memory and cannot observe when an object is reclaimed.

**Chars, enums and flags** are values, not references: they are copied like numbers.

## v2.5.2 Evaluation order

In addition to § 10.1, always from left to right:

- the fields of a record literal, in declaration order (which is the written order, § v2.4.4), then the record is created;
- the elements of a list literal or of `tuple(…)`, then the list or tuple is created;
- the payload of a constructor, then the value is created;
- in `e.f`, `e`; in `e[i]`, `e` then `i`, then the bounds are checked;
- in an assignment `p = v;`, the object and index designated by `p` first (from left to right), then `v`, then the bounds are checked and the value is stored;
- the operands of `+` on strings, then the new string is created;
- in `match (e)`, `e` once, then the block of its case runs.

## v2.5.3 Creation

Each evaluation of a record literal, a non-empty list literal, a tuple, a constructor with a payload, `+` on strings, `list::filled`, `list::copy`, `list::freeze`, `string::chars`, `string::from_chars` and `string::from_bytes` (when it returns `some`) creates a **new** object. Two evaluations of the same literal create two distinct objects.

String literals, constructors without payload and empty lists may be shared by the implementation: nothing can modify them, so the difference cannot be observed.

## v2.5.4 Values at the component boundary

When an exported function is called, its arguments come from **outside** the component (the host or another component), and its result goes outside. These values are always **deep copies** (chapter v2.7):

- the function receives new objects, which nobody outside holds: changing a `mut` field of an argument never changes the caller's value;
- the caller receives a copy of the result: the component can keep and change the original afterwards (in a global, for instance) without affecting what the caller received;
- sharing is not preserved: a record that appears twice in a returned list arrives as two distinct, equal records;
- `mut` is not visible outside: a `list<mut T>` is received as a WIT `list<T>`, a `mut` field as a plain field.

Calls between functions of the same component never copy, including calls to functions of exported interfaces: `total(inv)` inside `mark_paid` receives the same record.

## v2.5.5 Traps

New causes of traps, in addition to § 10.7:

| Cause | Message reported by wasmtime |
|---|---|
| index outside a list: `xs[i]` or `xs[i] = e` with `i >= list::len(xs)` | `out of bounds array access` |
| object too large: `list::filled(n, x)`, `+` or `list::copy` above the size limit of the runtime | `allocation size too large` |
| heap exhausted | the runtime's out-of-memory message |

Nothing else traps: there is no null reference, no failing cast, and no invalid UTF-8 (`string::from_bytes` returns `option::none`). Messages are those of the reference runtime; their exact text is not part of the language.

A trap during an exported call discards the whole instance (§ 10.7), with its heap.
