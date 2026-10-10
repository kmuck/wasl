# 16. Beyond v0.1

This chapter is not part of the v0.1 language. It records the direction already chosen, so that v0.1 is read in context and later versions extend it consistently. Nothing here is a commitment on syntax.

## 16.1 Direction

- **Latest standards only.** WaSL targets WASI 0.3 and the newest version of the Component Model, not older ones. In WASI 0.3, entry points are asynchronous (`run: async func() -> result`, `handle: async func(request: request) -> result<response, error-code>`), so `async` is a prerequisite for any program that uses WASI.
- **WIT-like structure.** Worlds and interfaces keep the WIT syntax: no shorthand that hides what a world exports or imports.
- **One world, one component, one entry point.** Foreign interfaces are implemented per world (§ 2.4.2).
- **A small core, rich libraries.** Productivity for real projects (HTTP routing, JSON, console output, database access) comes from libraries written in WaSL on top of WASI, and from the tooling, not from special cases in the compiler.
- **Example-driven design.** Before specifying a version, its target programs are written in full (in `examples/`), and the syntax is adjusted on them.

A reference use case for these versions: an enterprise backend made of several services (`account`, `order`, `product`…) in a **single project**, one world per service, each implementing `wasi:http/handler`, all sharing the same types and business logic, possibly with a `gateway` world that routes to all of them.

## 16.2 Planned features

**Data on the WasmGC heap.** `string`, `char`, `record`, `tuple`, `enum`, `variant`, `flags`, `list<T>`, `option<T>`, `result<T, E>`, with their WasmGC representation:

- mutability declared where WasmGC declares it: per record field (`age: mut u32`) and per list element type (`list<mut T>`);
- reference semantics for records (as objects in Java or JavaScript); copies only happen at component boundaries;
- lists of fixed size, like Wasm arrays; growable `vec<T>` and `map<K, V>` provided by the language;
- recursive types allowed for internal types, forbidden for exported ones (the Component Model does not allow them).

**Async and WASI 0.3.** `async func`, `.await`, `stream<T>`, `future<T>`, compiled to state machines driven by the runtime. Implementation of `wasi:cli/run` and `wasi:http/handler` in worlds (§ 2.4.2).

**Dependencies and imports.** WIT dependencies declared in the manifest and fetched with `wkg`; `import` in worlds; direct calls to imported functions, without generated bindings.

**Resources.** `resource`, `own<T>`, `borrow<T>`, with automatic release at the end of the scope that owns a handle.

**Tooling.** `weasel run <world>`, `weasel serve <world>` (and a way to serve several worlds for local development), `weasel build --all`, `weasel test`, `weasel publish`; reporting several errors per compilation; a formatter.

**Literals and comments.** Hexadecimal and binary literals, digit separators, exponents; `///` documentation comments copied into the WIT.

## 16.3 Open questions

- How a world implements a foreign interface exactly (§ 2.4.2 gives the model), and how imported functions are called.
- An operator to propagate errors of `result` and `option` (such as `?`).
- How tests are declared and discovered by `weasel test`.
- How WaSL libraries are distributed: as source packages, or as components composed with `wac`.
- JSON: a library written in WaSL, plus code generated from WIT records by a tool outside the compiler.
- Optional overflow checks, in addition to wrapping arithmetic.
- The `gc` option of the Canonical ABI: passing WasmGC values across component boundaries without copies, once runtimes support it. Until then, values are copied through linear memory at the boundaries only.
