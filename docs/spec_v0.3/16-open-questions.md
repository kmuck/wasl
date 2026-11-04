# 16. Open questions

These questions were met while writing the target programs and the specification. Each one has a default in the chapters above; this list says where the default is weakest. Features already planned, such as imports, resources and async, are in [ROADMAP.md](../../ROADMAP.md).

1. **Error propagation.** `total` and `mark_paid` (§ 15.3) show how often a `result` is matched only to return its `err`. An operator such as `?` would remove most of these `match`es. It is planned for v0.3, now that `match` is designed.
2. **Qualified constructors.** Every case is written with its type (`status::paid`, `option::some(x)`, `result::ok(sum)`), even where the expected type already determines it. This keeps one way to write each value and makes code readable out of context, at the cost of length. The alternative is bare cases where a type is expected (`return paid;`). `case` already names cases without their type (§ 8.16).
3. **Field order in record literals.** Fields are written in declaration order (§ 8.11). It is easy to relax later and impossible to tighten, which is why the strict rule comes first.
4. **Equality of heap values.** `==` only applies to strings among the heap types (§ 8.3). Structural equality for records, lists, options and results is convenient but hides a deep traversal; identity equality (`ref.eq`) exposes something the boundary does not preserve (§ 10.10). Neither is in the language.
5. **The cost of `list::freeze`.** `list<T>` is really immutable (§ 7.10), so a list built element by element is copied once by `list::freeze`. A compiler could skip the copy when the mutable list is not used after `freeze`, but that is an optimization, not a rule of the language. The real fix is in WasmGC: a way to build an immutable array from values computed at run time (§ 12.5).
6. **Text formatting.** `text::from_u32` in § 15.3 is the kind of function every project needs. It belongs to the standard library, which does not exist yet; until then, projects write their own.
7. **Flags operators.** Flags have `|`, `&`, `==` and a test per flag (`d.paper`). There is no complement (`!`) or difference: they are easy to add if programs need them.
8. **Tests.** How tests are declared and discovered by `weasel test`.
9. **Libraries.** How WaSL libraries are distributed: as source packages, or as components composed with `wac`.
10. **JSON.** A library written in WaSL, plus code generated from WIT records by a tool outside the compiler.

**Settled.** The `gc` option of the Canonical ABI will let WasmGC references cross component boundaries; the adapters of chapter 13 then disappear, and so do the copies of § 10.10. Nothing that crosses the boundary can change (§ 7.11), so no program can depend on receiving a private copy, and the copies can disappear without changing any behavior.
