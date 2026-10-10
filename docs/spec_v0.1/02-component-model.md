# 2. Mapping onto the Component Model

This chapter explains how a WaSL program is structured and how that structure maps onto the Component Model. It is the key to the rest of the specification: WaSL does not invent its own module system, it uses the one of WIT.

## 2.1 Packages

- A WaSL **project** is a WIT **package**. Its identity `namespace:name@version` (e.g. `example:calc@0.1.0`) is declared in the manifest `wasl.toml` (§ 3.2).
- All the source files of the project belong to the package (§ 3.3). There is no import between files: everything declared in the package is visible everywhere in it.
- The top-level declarations of a package are `world`s, `interface`s and `global`s (chapter 6).

## 2.2 Interfaces

In WIT, an interface is a contract: a named group of function signatures. In WaSL, an `interface` block is **the contract and its implementation at the same time**:

```wasl
interface math {
  func fib(n: u32) -> u64 { … }
  func circle_area(r: f64) -> f64 { … }
}
```

- The signatures of the functions form the contract; their bodies form the implementation.
- An interface of the package is defined **once**: it has a single implementation, shared by every world that exports it.
- An interface exported by at least one world appears in the WIT of the package, with all its functions.
- An interface exported by no world is **internal**: its functions can be called from anywhere in the package, but they appear in no WIT and no other component can call them. This is how a package keeps private code: there is no `pub` or `private` keyword.

## 2.3 Worlds and components

A world describes a component: the interfaces it exports (and, in later versions, the interfaces it imports).

```wasl
world calculator {
  export math;
}
```

- **One world is one component.** Each world of the package is compiled separately into its own component, `build/<world>.wasm`. A package with three worlds produces three components.
- **A world is an entry point.** The functions a component offers to its host are exactly the functions of the interfaces its world exports.
- **A component is self-contained.** It contains the code of the functions its world exports, plus every function they call and every global they use, whatever interface they come from. Two components built from the same package each have their own copy of the code they share, and each instance of a component has its own copy of the globals.
- The whole package is analyzed before any world is built: an error anywhere in the package prevents the build of every world.

## 2.4 The two kinds of exports

A world can export two kinds of interfaces. The difference is **where the implementation is written**.

### 2.4.1 Interfaces of the package

```wasl
world calculator {
  export math;
}
```

`export math;` exports an interface declared in the package. Its implementation is the `interface math { … }` block: the same code in every world that exports it. This is how a package shares business logic between several components.

### 2.4.2 Interfaces of other packages (specified, not available in v0.1)

An interface can also be defined by **another package**: a WASI interface such as `wasi:cli/run` or `wasi:http/handler`, or any WIT interface fetched as a dependency with `wkg`. The package that defines it only provides the contract. Its implementation is written **inside the world that exports it**:

```wasl
world main {
  export math;
  export wasi:cli/run@0.3.0 {
    async func run() -> result {
      // the command-line program, which can call math::fib(…)
    }
  }
}

world test {
  export wasi:cli/run@0.3.0 {
    async func run() -> result {
      // a test program that calls the same math::… functions
    }
  }
}
```

Rules:

- The block after `export <package>:<namespace>/<interface>@<version>` implements that interface **for this world only**. Each world that exports it has its own implementation.
- The block must implement every function of the foreign interface, with exactly the signatures that its WIT declares, and nothing else.
- The functions of the block can call the functions of the package interfaces (`math::fib(…)`) and the other functions of the same block (by their name).
- A foreign interface can only be implemented in a world. An `interface` block always declares an interface of the package.

Why this model:

- **It is the Component Model.** A component carries its own implementation of each of its exports. Two components never share an implementation, even when they come from the same sources; packages only exist at the source level.
- **Entry points are foreign interfaces.** In WASI 0.3, a command-line program is a component that exports `wasi:cli/run` (`run: async func() -> result`), and an HTTP service is a component that exports `wasi:http/handler` (`handle: async func(request: request) -> result<response, error-code>`). Every language that targets WASI implements these interfaces; Rust and Go hide it behind `main`, WaSL writes it directly.
- **One project, several entry points.** A project can have a `main` world for its command-line tool and a `test` world that runs its tests, or one world per HTTP service (`account`, `order`, `product`…) sharing the same types and business logic. Each world needs its own `run` or `handle`: the implementation belongs to the world.

The WIT of the world `main` above is exactly:

```wit
world main {
  export math;
  export wasi:cli/run@0.3.0;
}
```

Status in v0.1: v0.1 has no dependencies, no WASI, no `async` and no `result` type, so a v0.1 compiler rejects this form. The `@` of the version is not a valid character in v0.1 (E0001); without a version, the qualified name after `export` is a syntax error (E0100).

## 2.5 Imports (specified, not available in v0.1)

A world will also **import** interfaces: interfaces whose implementation is provided by the host (WASI interfaces such as `wasi:filesystem` or `wasi:http/client`) or by another component. Their WIT is fetched as a dependency with `wkg`, like any other package, and WaSL code calls their functions directly, without generated bindings. The syntax of imports, the declaration of dependencies in the manifest and the calls to imported functions are defined by a later version. In v0.1, `import` is a reserved word.

## 2.6 The WIT of a package

The WIT of the package is **derived from the source code by erasure**:

- function bodies are removed, only signatures remain;
- globals are removed: they are internal state, never visible from outside (§ 6.4);
- internal interfaces are removed;
- names are converted from `snake_case` to WIT `kebab-case` (`circle_area` → `circle-area`, § 13.2).

```wasl
world calculator {
  export math;
}

global calls: mut u32 = 0;

interface math {
  func fib(n: u32) -> u64 { … }
  func circle_area(r: f64) -> f64 { … }
}

interface util {
  func clamp(x: s32, lo: s32, hi: s32) -> s32 { … }
}
```

```wit
package example:calc@0.1.0;

interface math {
  fib: func(n: u32) -> u64;
  circle-area: func(r: f64) -> f64;
}

world calculator {
  export math;
}
```

Each component embeds the WIT of its world: tools such as `wasm-tools component wit` read it back from the `.wasm` file. The source code is the only source of truth; no `.wit` file is written by hand. The exact generation rules are in chapter 13.
