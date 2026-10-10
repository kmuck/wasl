# 6. Declarations

A source file contains three kinds of top-level declarations: `world`, `interface` and `global`. Their role in the Component Model is described in chapter 2; this chapter gives their rules.

## 6.1 Package namespace

- The names of **all** the worlds, interfaces and globals of the package form a single namespace. Two top-level declarations cannot have the same name, even in different files or if they are of different kinds (E0201): a global cannot be named like an interface.
- A world or an interface is defined in **a single block**, in a single file. It cannot be split across several blocks or files (a second block with the same name is a duplicate, E0201).
- Function names live inside their interface (§ 6.3); local variable names live inside their function (chapter 9).

## 6.2 `world`

```wasl
world calculator {
  export math;
  export stats;
}
```

- A world contains only `export <interface>;` items.
- Each `export` names an interface of the package. An unknown name, or the name of a world or a global, is an error (E0203).
- An interface can be exported only once per world (E0204). The same interface can be exported by several worlds.
- A world can be empty; it then produces a component without exports.
- Each world is compiled into its own component (§ 2.3).
- Exporting an interface of another package, with its implementation in the world, is specified in § 2.4.2 and not available in v0.1.

## 6.3 `interface`

```wasl
interface math {
  func fib(n: u32) -> u64 { … }
  func reset() { … }
}
```

- An interface contains only functions (`func`). It can be empty.
- Two functions of the same interface cannot have the same name (E0202). There is no overloading. Two different interfaces can have functions with the same name.
- All the functions of an interface are public: if the interface is exported by a world, all of them appear in the WIT.
- An interface exported by no world is internal (§ 2.2).

## 6.4 `global`

```wasl
global pi: f64 = 3.14159;
global calls: mut u32 = 0;
global offset: s32 = -40;
global debug: bool = false;
```

- A global is declared at the top level, never inside an interface.
- It is **visible from every function of every interface of the package** (§ 9.2).
- Its type is required. `mut` makes it assignable; without `mut`, assigning it is an error (E0500).
- Its initial value is a literal of its type, optionally preceded by `-` (§ 8.2): `u8 = 300` is out of range (E0401), `f64 = 1` uses an integer literal for a float type (E0400), `u32 = -1` applies `-` to an unsigned type (E0403). Wasm requires the initial value of a global to be a constant, which is why no other expression is allowed.
- A global lives as long as the instance of the component: a `mut` global keeps its value between two calls on the same instance, and each instance starts with the initial value.
- A global belongs to the components that use it: two components built from the same package each have their own copy (§ 2.3).
- A global never appears in the WIT. Another component can neither read nor modify it; to expose a value, write a function that returns it.

## 6.5 `func`

```wasl
func clamp(x: s32, lo: s32, hi: s32) -> s32 { … }
func reset() { … }
```

- A function has zero or more parameters and zero or one return value. The type of each parameter and of the return value is required.
- Two parameters of the same function cannot have the same name (E0304).
- A function is declared inside an interface; there are no free functions at the top level.
- The rules of the body are in chapters 8 (typing) and 9 (names).
- A function of an **exported** interface (exported by at least one world) has **at most 16 parameters** (E0205). Beyond 16 values, the Canonical ABI passes parameters through linear memory, which v0.1 components do not have (§ 13.4). Functions of internal interfaces have no limit.
- Functions can call each other freely, including recursively and across interfaces.
