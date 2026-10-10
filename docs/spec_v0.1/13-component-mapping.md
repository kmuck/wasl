# 13. Mapping to the Component Model

This chapter specifies the WIT of a package and how the core module of a world (chapter 12) becomes a component. The principles are in chapter 2.

## 13.1 Generated WIT

The **WIT of the package** contains, in this order:

1. the declaration `package <namespace>:<name>@<version>;` from the manifest (§ 3.2);
2. every interface exported by **at least one** world, in declaration order, with all its functions in declaration order;
3. every world, in declaration order, with its `export`s.

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
- A world `world w { export i; }` becomes `world w { export i; }`.
- `weasel wit` prints the WIT of the package (§ 14.2). The layout of the printed text (indentation, blank lines) is not specified; its content is.
- The **component of a world** embeds the WIT of that world: the world, the interfaces it exports and the package they belong to.

## 13.2 Names

- A WaSL identifier becomes a WIT identifier by replacing each `_` with `-`: `fib_rec` → `fib-rec`, `circle_area` → `circle-area`, `get_calls` → `get-calls`. The identifier rules of § 4.3 guarantee that the result is a valid WIT identifier and that two different WaSL names never give the same WIT name.
- This applies to interfaces, worlds, functions and parameters.
- The v0.1 types have the same names in WaSL and in WIT (§ 7.1).
- A WaSL identifier may coincide with a WIT keyword that is not reserved in WaSL (for instance `from`). The WIT then writes it with the WIT escape `%` (`%from`), which is the standard way to use a keyword as a name in WIT.

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

- Every exported function is *lifted* without canonical options: no `memory`, no `realloc`, no `post-return`, no `string-encoding` is needed, because v0.1 only exchanges scalars.
- Each parameter and the result are passed directly as core values:

| WaSL / WIT type | Core type |
|---|---|
| `bool`, `s8`, `s16`, `s32`, `u8`, `u16`, `u32` | `i32` |
| `s64`, `u64` | `i64` |
| `f32` | `f32` |
| `f64` | `f64` |

- The Canonical ABI passes at most 16 parameters as core values; beyond, they go through linear memory. This is why exported functions are limited to 16 parameters (§ 6.5).
- When the host passes a `bool`, `s8`, `u8`, `s16` or `u16`, the Canonical ABI already gives the core function a normalized `i32` (§ 12.2); when the core function returns one, the component reads the low-order bits.

## 13.5 Building the component

The reference implementation builds the component in three steps:

1. it generates the core module of the world (chapter 12);
2. it attaches the WIT of the world to the module, as a `component-type` custom section (`wit_component::embed_component_metadata`);
3. it wraps the module into a component with `wit_component::ComponentEncoder`, which reads that section, checks that every exported function exists with the right core signature, generates the lifts and validates the result.

The result is a standard component:

```sh
wasm-tools validate build/calculator.wasm
wasm-tools component wit build/calculator.wasm     # prints the WIT of the world
wasm-tools print build/calculator.wasm             # prints the component as text
```
