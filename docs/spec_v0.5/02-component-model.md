# 2. Mapping onto the Component Model

This chapter explains how a WaSL program is structured and how that structure maps onto the Component Model. It is the key to the rest of the specification: WaSL does not invent its own module system, it uses the one of WIT.

## 2.1 Packages

- A WaSL **project** is a WIT **package**. Its identity `namespace:name@version` (e.g. `example:calc@0.1.0`) is declared in the manifest `wasl.toml` (§ 3.2).
- All the source files of the project belong to the package (§ 3.3). There is no import between files: everything declared in the package is visible everywhere in it. A file names the interfaces of **other** packages it uses with `use` (§ 6.6).
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

### 2.4.2 Interfaces of other packages

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
- The block must implement every function of the foreign interface, with exactly the signatures that its WIT declares, and nothing else (E0218).
- Inside the block, the names of the exported interface are written **without path**, as inside an interface of the package: its types, the types it takes from other interfaces with a WIT `use` (`request` in `wasi:http/handler`), and its functions. `types::request` stays valid when the file has `use wasi:http/types;` (§ 6.6).
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

### 2.4.3 Functions without body and `include`

*Specified, not available yet.*

A **library** is a dependency written in WaSL whose source code is compiled with the package (§ 3.5). Two things let a library write an entry point once, for every program:

- an interface of the library can declare a function **without body**: its signature followed by `;`. The program writes its body;
- the library can declare a **world**, which a world of the program **includes**. `include` is the `include` of WIT: the imports and the exports of the included world become those of the world that includes it.

The standard library uses both for HTTP services and commands (chapter 17):

```wasl
// in wasl:std (chapter 17), simplified
interface http {
  record request { method: method, path: string, … }
  record response { status: u16, … }

  async func handle(request: request) -> response;      // without body: written by the program

  func reply(status: u16, content_type: string, body: string) -> response { … }
}

world service {
  export wasi:http/handler {
    async func handle(req: types::request) -> result<types::response, types::error_code> {
      local answer = http::handle(receive(req));        // calls the function of the program
      local (response, body) = start(answer);
      task.return(result.ok(response));
      finish(body, answer);
    }
  }
}
```

```wasl
// in the program
world catalog {
  include wasl:std/service {
    async func handle(request: request) -> response {
      if (on(request, method::get, "/products"))
        return reply(200, "application/json", products::all());
      return not_found();
    }
  }
}
```

Rules:

- Only an interface of a library can declare a function without body. In an interface of the package, it is an error (E0224): the package would have no implementation for it.
- `include <foreign name>;` names a world of a library (E0215). The world gets the imports and the exports of the included world, as if they were written at the place of the `include`, with the code of their export blocks, which belongs to the library. A world includes at most one world in 0.5 (E0204), and cannot export or import an interface that the included world already exports or imports (E0204, E0216).
- The block after `include` contains exactly the functions without body that the code of the included world reaches (§ 12.1), with their signatures (E0218). When there is none, the item is written `include <foreign name>;`.
- Inside a function of the block, the names of the interface that declares the function are written without path, as in a foreign export block (§ 2.4.2): `request`, `response`, `reply`, `on`.
- A function without body is called by the code of the library only, like any function: `http::handle(…)`. It has no name in the package, so the package cannot call it (E0301), and a test cannot either (§ 3.6).
- A world of a library is only a part of the worlds that include it: `weasel` never builds it alone, and the interfaces of a library cannot be exported by a world of the package (E0218).
- In the WIT, the `include` is expanded: the world of the program is written with the imports and the exports of the included world (§ 13.1). The function without body does not appear: it is not exported.

**In the binary.** One component. The export blocks of the included world are compiled into it like the export blocks of the world itself, with the functions of the library that they reach (§ 3.5), and each function of the `include` block is an internal function of the module, which the code of the library calls with a plain `call`. The values that cross are WasmGC references: nothing is copied, and no other component or instance is involved. Two worlds that include the same world each compile their own function into their own component.

## 2.5 Imports

A world can also **import** interfaces: interfaces whose implementation is provided by the host (WASI interfaces such as `wasi:cli/stdout` or `wasi:filesystem/types`) or by another component.

```wasl
use example:catalog/carts;

world checkout {
  import example:catalog/carts;
  export orders;
}

interface orders {
  func quote(sku: string, quantity: u32) -> result<u64, carts::cart_error> {
    local c: carts::cart = carts::cart::of(sku, quantity)?;
    return result.ok(c.total());
  }
}
```

There are two kinds of imports:

- **An interface of another package**, `import <namespace>:<package>/<interface>;`, written like a foreign export (§ 2.4.2), from a dependency of the project (§ 3.5). A file that names it has `use <namespace>:<package>/<interface>;` (§ 6.6), and then names its functions and types with the last part of its name as path: `carts::cart`, `stdout::get_stdout()`. WaSL code calls them directly, without generated bindings.
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
- In one file, two interfaces that code can name, the interfaces of the package and the interfaces of other packages that the file uses, cannot have the same name (E0217): a file that has `use example:catalog/carts;` cannot be in a package with an `interface carts`. `use … as` gives the foreign interface another name in the file: `use wasi:http/types as http_types;` (§ 6.6).
- A function reachable from the exports of a world that calls a function of another package must belong to a world that imports that interface (E0221). An interface of the package that a world does not import is compiled into its component, as in § 2.3.
- An imported interface has no implementation in the component: its functions are imported functions of the core module (§ 13.10). Its resources are **handles** (§ 7.12).
- An item of an interface of another package that WaSL cannot use, because its name gives no WaSL identifier (§ 13.2), or because it uses a WIT type WaSL does not have (`map`, `error-context`, or a `stream` or `future` without an element type), can be named, but naming it is an error (E0215) that says why. The other items of the interface stay usable. Async functions can be used (§ 2.8). The types `stream<T>` and `future<T>` can be used too (§ 7.13).
- A type alias of another package (`type instant = u64;`) is named like a declared type (`clocks::instant`) and designates the type it aliases.

Why this model:

- **It is the Component Model.** A world lists its imports and its exports; WIT names them the same way. The component carries the code of its exports and nothing of its imports.
- **The two kinds are the two ways to reuse code.** Without `import`, an interface of the package is a library: its code is copied into each component that uses it (§ 2.3). With `import`, it is a service: one component provides it, the others call it at run time, with their own instance and their own state.
- **No version in the source.** A dependency has one version, written once in the manifest (§ 3.2); the generated WIT writes it (§ 13.1), so the source and the manifest cannot disagree.
- **`import` and `use` are the two words of WIT.** In WIT too, a world `import`s an interface, which becomes an import of the component, and a file or an interface `use`s one, which only gives it a name. WaSL keeps them apart for the same reason: naming an interface never changes the binary.

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

## 2.7 Resources

A **resource** is the Component Model's object: a type with an identity, a private state and methods, which other components reach only through a **handle**. WaSL declares resources in interfaces, like WIT (§ 7.12):

- a resource **of the package** is a WasmGC object inside its component, like a record whose fields nobody else can see; a handle to it is created only when it leaves the component;
- a resource of an **imported** interface lives in another component; the component only holds handles to it, which the compiler gives back to their owner when the code no longer needs them (§ 9.9).

## 2.8 Async functions and tasks

Async functions, streams, futures and `task.return` are available.

WASI 0.3 makes input and output async: `wasi:cli/run` declares `run: async func() -> result`, and `wasi:http/handler` declares `handle: async func(request: request) -> result<response, error-code>`. WaSL takes the async model of the Component Model as it is:

```wasl
use wasi:cli/types;
use wasi:cli/stdout;

world hello {
  import wasi:cli/types;
  import wasi:cli/stdout;

  export wasi:cli/run {
    async func run() -> result {
      local (rx: stream<u8>, tx: stream_writer<u8>) = stream.new();
      local done: future<result<_, types::error_code>> = stdout::write_via_stream(rx);
      stream.write(tx, string.bytes("Hello, world!\n"));       // waits until stdout has taken the bytes
      stream.close(tx);
      match (future.read(done)) {                             // waits until stdout is done
        case ok { return result.ok; }
        case err { return result.err; }
      }
    }
  }
}
```

- **Tasks.** Each call of an exported function is a **task** of the Component Model. A task delivers its result to its caller once. An async task can deliver it before its end, with `task.return`, and go on running: an HTTP handler returns its response, then writes its body (§ 8.21).
- **`async func`** declares an async function, the `async func` of WIT, wherever WIT allows one: in an interface, in a resource (`async func m()`, `static async func s()`) and in a foreign export block. A constructor is never async, as in WIT.
- **A call waits.** A call to an async function returns when the callee has delivered its result: `client::send(req)` is written like any other call, without `await`. This is the synchronous lowering of the Component Model, where the caller is suspended until the result is there.
- **Colors come from WIT.** Only an async function can call an async function, an async method or an async built-in function (E0416). The Component Model traps when a task that is not async would block; WaSL reports it at compile time, with one check per call. Functions that are not async can be called from anywhere.
- **Suspension points.** While a call to an async function waits, other tasks of the same instance may run (§ 10.12): a `mut` global read before the call may have changed after it. A call to a function that is not async is never a suspension point.
- **Streams and futures** carry data between components while their tasks run: the body of an HTTP request, the standard output of a command (§ 7.13).

Why this model:

- **It is the Component Model.** A WaSL call is a `canon lower`, and the Component Model has no `await`, so neither does WaSL. The color of a function is part of its WIT type, which the binary keeps, so the source keeps it too.
- **Programs do not depend on how tasks are scheduled.** In 0.4, a component never runs two async tasks at the same time, and `weasel serve` gives each concurrent request its own instance (§ 13.13). The specification still says that other tasks may run at every async call, so that a later compiler can run many tasks in one instance, with the stackful or threaded modes of the Component Model, without changing any program (§ 16).
- **Concurrency inside a task comes later.** `async f(x)` will start a call without waiting for it and return a *subtask*, the word of the Component Model (§ 16). A call written without `async` always waits, so a forgotten marker never starts something by accident.

## 2.9 The standard library

*Specified, not available yet.*

The standard library is a WaSL package, `wasl:std`, shipped with `weasel` and versioned with the compiler. It sits above WASI, as the standard library of other languages sits above the system calls: a program writes `print("…")` and `reply(200, …)`, and the library speaks `wasi:cli` and `wasi:http`. Its interfaces are `text`, `json`, `http`, `cli` and `fs` (chapter 17).

```wasl
use wasl:std/cli;

world hello {
  include wasl:std/command {
    async func run() -> result<_, string> {
      if (list.len(args()) < 2)
        return result.err("usage: hello <name>");
      print("Hello, " + args()[1] + "!\n");
      return result.ok;
    }
  }
}
```

- **A library compiled in.** The interfaces of `wasl:std` are compiled into each component that uses them, like the interfaces of the package (§ 3.5). A program pays only for what it calls.
- **Entry points.** The library declares two worlds, `service` and `command`, which export `wasi:http/handler` and `wasi:cli/run`, and two functions without body, `handle` in `wasl:std/http` and `run` in `wasl:std/cli` (§ 2.4.3). A world that includes `wasl:std/service` and writes `handle` is an HTTP service; one that includes `wasl:std/command` and writes `run` is a command. The code of the library is compiled into the same component and calls the function of the program: the built file is a plain WASI 0.3 service or command, in one component (§ 17.1).
- **WASI stays available.** A program can still import WASI interfaces and export `wasi:http/handler` or `wasi:cli/run` itself, for what the library does not cover, such as streaming a body.
