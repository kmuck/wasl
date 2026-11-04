# 2. Mapping onto the Component Model

This chapter explains how a WaSL program is structured and how that structure maps onto the Component Model. It is the key to the rest of the specification: WaSL does not invent its own module system, it uses the one of WIT.

## 2.1 Packages

- A WaSL **project** is a WIT **package**. Its identity `namespace:name@version` (e.g. `example:calc@0.1.0`) is declared in the manifest `wasl.toml` (§ 3.2).
- All the source files of the project belong to the package (§ 3.3). There is no import between files: everything declared in the package is visible everywhere in it.
- The top-level declarations of a package are `world`s, `interface`s and `global`s (chapter 6). Types are declared inside interfaces (§ 7.7).

## 2.2 Interfaces

In WIT, an interface is a contract: a named group of types and function signatures. In WaSL, an `interface` block is **the contract and its implementation at the same time**:

```wasl
interface math {
  func fib(n: u32) -> u64 { … }
  func circle_area(r: f64) -> f64 { … }
}
```

- The types and the signatures of the functions form the contract; the bodies of the functions form the implementation.
- An interface of the package is defined **once**: it has a single implementation, shared by every world that exports it.
- An interface exported by at least one world appears in the WIT of the package, with all its types and functions.
- An interface exported by no world is **internal**: its functions can be called from anywhere in the package, but they appear in no WIT and no other component can call them. This is how a package keeps private code: there is no `pub` or `private` keyword.

## 2.3 Worlds and components

A world describes a component: the interfaces it imports (§ 2.5) and the interfaces it exports.

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

### 2.4.2 Interfaces of other packages (specified, not available yet)

An interface can also be defined by **another package**: a WASI interface such as `wasi:cli/run` or `wasi:http/incoming-handler`, or any WIT interface of a dependency (§ 3.5). The package that defines it only provides the contract. Its implementation is written **inside the world that exports it**:

```wasl
world main {
  export math;
  export wasi:cli/run {
    func run() -> result {
      // the command-line program, which can call math::fib(…)
    }
  }
}

world test {
  export wasi:cli/run {
    func run() -> result {
      // a test program that calls the same math::… functions
    }
  }
}
```

Rules:

- The name after `export` is the name of the interface in WIT, `<namespace>:<package>/<interface>`, written with WaSL identifiers: `wasi:http/incoming_handler` for `wasi:http/incoming-handler` (§ 13.2). It has no version: the version is the one of the dependency, declared in the manifest (§ 3.2). The package must be a dependency of the project (E0215).
- The block after the name implements that interface **for this world only**. Each world that exports it has its own implementation.
- The block must implement every function of the foreign interface, with exactly the signatures that its WIT declares, and nothing else (E0218). The types of the interface are named with the interface's own name as path (`types::request`), as for an imported interface (§ 2.5).
- The functions of the block can call the functions of the package interfaces (`math::fib(…)`), the functions of the interfaces the world imports, and the other functions of the same block (by their name).
- A foreign interface can only be implemented in a world. An `interface` block always declares an interface of the package.

Why this model:

- **It is the Component Model.** A component carries its own implementation of each of its exports. Two components never share an implementation, even when they come from the same sources; packages only exist at the source level.
- **Entry points are foreign interfaces.** A command-line program is a component that exports `wasi:cli/run` (`run: func() -> result` in WASI 0.2, an `async func` in WASI 0.3), and an HTTP service is a component that exports the handler of `wasi:http`. Every language that targets WASI implements these interfaces; Rust and Go hide it behind `main`, WaSL writes it directly.
- **One project, several entry points.** A project can have a `main` world for its command-line tool and a `test` world that runs its tests, or one world per HTTP service (`account`, `order`, `product`…) sharing the same types and business logic. Each world needs its own `run` or `handle`: the implementation belongs to the world.

The WIT of the world `main` above, with the dependency `"wasi:cli" = "0.2.6"`, is exactly:

```wit
world main {
  export math;
  export wasi:cli/run@0.2.6;
}
```

## 2.5 Imports (specified, not available yet)

A world can also **import** interfaces: interfaces whose implementation is provided by the host (WASI interfaces such as `wasi:cli/stdout` or `wasi:filesystem/types`) or by another component.

```wasl
world checkout {
  import example:catalog/carts;
  export orders;
}

interface orders {
  func quote(sku: string, quantity: u32) -> result<u64, carts::cart_error> {
    local c: carts::cart = carts::cart::of(sku, quantity)?;
    return result::ok(c.total());
  }
}
```

There are two kinds of imports:

- **An interface of another package**, `import <namespace>:<package>/<interface>;`, written like a foreign export (§ 2.4.2), from a dependency of the project (§ 3.5). Its functions and types are named with the last part of its name as path: `carts::cart`, `stdout::get_stdout()`. WaSL code calls them directly, without generated bindings.
- **An interface of the same package**, `import <interface>;`. In the component of this world, the interface is not compiled in: every call to its functions goes to another component, which provides it, usually the component of another world of the package that exports it. This is how the services of one project call each other while each one is deployed, scaled and instantiated on its own:

```wasl
world accounts_service {
  export accounts;
}

world orders_service {
  import accounts;      // calls to accounts::… leave the component
  export orders;
}
```

Rules:

- A world imports an interface at most once, and cannot both import and export it (E0216).
- Two interfaces that code can name, the interfaces of the package and the interfaces of other packages that a world of the package imports or exports, cannot have the same last name (E0217): `example:catalog/carts` cannot coexist with an `interface carts` of the package.
- A function reachable from the exports of a world that calls a function of another package must belong to a world that imports that interface (E0221). An interface of the package that a world does not import is compiled into its component, as in § 2.3.
- An imported interface has no implementation in the component: its functions are imported functions of the core module (§ 13.10). Its resources are **handles** (§ 7.12).

Why this model:

- **It is the Component Model.** A world lists its imports and its exports; WIT names them the same way. The component carries the code of its exports and nothing of its imports.
- **The two kinds are the two ways to reuse code.** Without `import`, an interface of the package is a library: its code is copied into each component that uses it (§ 2.3). With `import`, it is a service: one component provides it, the others call it at run time, with their own instance and their own state.
- **No version in the source.** A dependency has one version, written once in the manifest (§ 3.2); the generated WIT writes it (§ 13.1), so the source and the manifest cannot disagree.

## 2.6 The WIT of a package

The WIT of the package is **derived from the source code by erasure**:

- function bodies are removed, only signatures remain;
- globals are removed: they are internal state, never visible from outside (§ 6.4);
- the types of the interfaces that appear are kept, without `mut`, which the WIT does not have (§ 7.11);
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

Each component embeds the WIT of its world: tools such as `wasm-tools component wit` read it back from the `.wasm` file. The source code is the only source of truth; no `.wit` file is written by hand. The exact generation rules are in § 13.1, including the interfaces that only appear because another one uses their types.

## 2.7 Resources (specified, not available yet)

A **resource** is the Component Model's object: a type with an identity, a private state and methods, which other components reach only through a **handle**. WaSL declares resources in interfaces, like WIT (§ 7.12):

- a resource **of the package** is a WasmGC object inside its component, like a record whose fields nobody else can see; a handle to it is created only when it leaves the component;
- a resource of an **imported** interface lives in another component; the component only holds handles to it, which the compiler gives back to their owner when the code no longer needs them (§ 9.9).
