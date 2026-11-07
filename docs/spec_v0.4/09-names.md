# 9. Names and scopes

## 9.1 Functions

- Inside an interface, a function of the **same interface** is called by its name: `fib(n)`.
- A function of **another interface** of the package is called through a path: `math::fib(n)`. The path names an interface of the package; the current interface can also be named this way.
- Calling an unknown function is an error (E0301). If the name exists in another interface, the error message suggests the path.
- In a path `i::f`, `i` must be an interface: an unknown name, or the name of a world or a global, is an error (E0300).
- A function of an interface of another package is called with the last part of the interface name as path, `stdout::get_stdout()`, from a function that belongs to a world that imports the interface (E0221, § 2.5).
- Inside the block of a foreign export (§ 2.4.2), a function of the same block is called by its name; the functions of the interfaces of the package and of the imported interfaces through a path.
- Functions are not values: a function name only appears in a call. It cannot be stored, passed or returned.
- Recursion is allowed, including mutual recursion and recursion across interfaces.

## 9.2 Globals

A global is designated by its name, from any function of any interface of the package: `calls = calls + 1;`. Globals are not qualified by a path.

## 9.3 Resolution of a name in an expression

A name used as a value is looked up:

1. among the parameters and locals visible at that point, from the innermost scope outwards;
2. then among the globals of the package.

Otherwise it is an error (E0300). Names of interfaces, worlds, functions and types are not values: `x = math;` is an unknown name. Enum cases and constructors are always written with a path (§ 9.8): `return paid;` is an unknown name, write `status::paid`.

## 9.4 Parameters and locals

- Parameters are **locals**, as in Wasm: they are visible in the whole body of the function and they can be **assigned**.
- A `local` is visible from the end of its declaration to the end of the block that contains it. Its initial value cannot use it: in `local q: u32 = q;`, `q` is unknown (E0300) unless it designates a global, which is itself forbidden (next rule).
- A parameter or a local **cannot have the name of a global** (E0303).
- A parameter or a local cannot have the name of another parameter or local that is visible at that point (E0304). There is no shadowing. Two sibling blocks (neither inside the other) can each declare a local with the same name:

```wasl
if (b) {
  local t: u32 = 1;
} else {
  local t: u32 = 2;     // valid: the first `t` is not visible here
}
```

- A local can have the name of an interface, a world, a function or a type: they are never confused, since those names are only used in paths, types, `export` items and calls.
- A binding of a `match` case (`case some(x)`) is a local of the block of that case (§ 8.16), with the same rules.
- A local declared without an initial value starts at the **default value of its Wasm type**: the value Wasm gives a local before its first `local.set`. Only scalars and options have one:

| Type | Wasm type (§ 12.5) | Default value |
|---|---|---|
| integers, floats, `bool` | `i32`, `i64`, `f32`, `f64` | `0`, `0.0` (positive zero), `false` |
| `char` | `i32` | `'\u{0}'` |
| an enum | `i32`, the index of the case | its first case |
| a flags type | `i32` | the empty set |
| `option<T>` | a nullable reference | `option::none` |

- A string, a list, a tuple, a record, a variant or a result is a non-nullable reference in Wasm, which has no default value: a local of one of these types must have an initial value (E0410). The language still has no `null` (§ 1.3): the null reference of an option is its `none`, a value of the option type.
- The default value is set every time the declaration runs: a local declared inside a loop starts from it again on every iteration. The default value of an enum depends on the order of its cases, as its representation does.

```wasl
local i: u32;               // 0
local s: status;            // status::draft, the first case
local d: option<discount>;  // option::none
local t: string;            // E0410: a string needs an initial value
```

- A **destructuring** `local (a: A, b: B, …) = e;` declares one local per element of the tuple `e`:

```wasl
local (count: u32, total: u64) = stats(lines);
local (rx: stream<u8>, tx: stream_writer<u8>) = stream::new();
local (response: types::response, _) = types::response::new(headers, option::some(body), trailers);
```

- `e` must be a tuple with one element per position (E0400). Each position is a name with its type, which is the type of the element (E0400), or `_`, which keeps nothing. The expected type of `e` is the tuple of the written types; a `_` gives no expected type for its position (§ 8.2).
- The locals follow the rules of every local: they are visible from the end of the declaration, cannot have the name of a global (E0303) or of a visible local (E0304), and two positions cannot have the same name (E0304).
- A position written `_` is released at once when its element holds handles (§ 9.9). `types::response::new` returns, with the response, a future that reports when the body has been sent; a handler that does not need it writes `_`.
- Destructuring is the only way to take handles out of a tuple, which is a temporary when it holds one (§ 7.12). A tuple without handles can also be kept in one local and read with `t.0` (§ 8.12), which reads an element and declares nothing.

## 9.5 Assignment

The target of an assignment `p = e;` (§ 5.3) is:

- a parameter or a local visible at that point;
- a global declared `mut`;
- `x.f`, where `f` is a `mut` field of the record `x`, or of the resource `x` in the interface that declares it (§ 7.12), including `self.f`;
- `x[i]`, where `x` has type `list<mut T>`.

Assigning an unknown name is an error (E0300); assigning a global that is not `mut` is an error (E0500). Assigning a field that is not `mut`, an element of a `list<T>`, an element of a tuple or a flag of a flags value is E0505. To change flags, assign the whole value: `d = d | delivery { urgent };`. Assigning a field of a record stored in an immutable global is allowed when the field is `mut`: the global designates the record, not its contents. The value `e` has the type of the target (§ 8.1).

## 9.6 Calls used as statements

A call statement (§ 5.3), a call, a method call or a `?` followed by `;`, is valid only if it has no value: its function returns no value, or its `?` unwraps a `result<_, E>` (§ 8.19). Calling a function that returns a value as a statement is an error (E0413): the value would be lost without a trace, and with it the error of a `result`. To discard it on purpose, write `_ =` before the call.

```wasl
reset();          // a function without a return value
_ = incr();       // a function that returns a u32: the value is discarded
save(inv);        // E0413: `save` returns a result<_, save_error>, which would be lost
save(inv)?;       // valid: the error is propagated, and `ok` has no value
c.add(x);         // a method without a return value
```

This is what Wasm itself requires: a value left on the stack must be removed with `drop`, and `_ = f();` is that `drop`, written in the source (§ 12.8).

- Only a call statement can follow `_ =` (§ 5.3): `_ = x;` is E0100.
- `_ =` releases the handles that the discarded value contains (§ 9.9).
- `_ = f();` on a function without a return value is E0400, as any other use of its call as a value (§ 8.6).
- The built-in functions all return a value, except `stream::close` (§ 8.21): `string::len(s);` is E0413, `stream::close(tx);` is valid.

## 9.7 `break` and `continue`

- `break` and `continue` are only allowed inside a `while` (E0501).
- They act on the **innermost** enclosing `while`: `break` exits it, `continue` jumps to its next iteration (the condition is evaluated again).
- A `break` inside an `if` or a `match` inside a loop exits the loop, not the `if` or the `match`.

## 9.8 Paths

A path `a::b` or `a::b::c`, as an expression or before a call, is resolved by the kind of its first name:

| First name | Meaning | Examples |
|---|---|---|
| `string`, `list`, `char`, an integer type, `stream`, `future` | a built-in function (§ 8.17, § 8.21) | `string::len(s)`, `list::filled(3, 0)`, `char::from_u32(n)`, `u32::wrapping_add(h, c)`, `stream::new()` |
| `option`, `result` | a constructor of the built-in type | `option::none`, `result::ok(sum)` |
| a type of the current interface | a case of that enum or variant | `status::paid`, `discount::fixed(500)` |
| a resource of the current interface | its static function (§ 8.20) | `cart::single(150)` |
| an interface, followed by one name | a function of that interface (§ 9.1), or the constructor of its resource | `pricing::apply(sum, d)`, `carts::cart()` |
| an interface, followed by two names | a case of a type of that interface, or a static function of its resource | `invoices::status::paid`, `carts::cart::of("pen", 2)` |

An interface is an interface of the package, or an interface of another package that a world of the package imports or exports, named by the last part of its name (§ 2.5).

An unknown first name is E0300 (§ 9.1); a path that names a type that is not an enum or a variant, or an unknown case, is E0307; a case written with the wrong payload is E0407 (§ 8.14). Types are named with paths of the same form (§ 7.8).

## 9.9 Handles

A handle (§ 7.12) designates a resource that belongs to another component. The Component Model requires each handle to be given back to its owner, with `resource.drop`, exactly once; a handle that is never given back keeps the resource alive in the other component until the instance ends. WaSL gives handles back automatically, with rules that are checked locally, statement by statement.

**One owner.** At any time, a handle is owned by one place: a local (including a parameter of type `T` and a match binding), or a temporary (§ 7.12). Records, lists, globals and fields cannot hold handles, and a local cannot hold an `option`, a `result`, a variant or a tuple that holds one (E0220), so these places are all there is.

**Moves.** Using a local `x` of a handle type as a value **moves** the handle out of it:

- `return x;`, `return result::ok(x);`, and every constructor that holds `x` (`option::some(x)`, `result::ok(x)`, a variant case);
- an argument for a parameter of type `T` (not `borrow<T>`);
- `local y: T = x;` and `y = x;`;
- the end given to `stream::close`, `future::write` or `future::read`, and the value given to `task::return` (§ 8.21; *specified, not available yet*).

After a move, `x` holds the empty handle, `0`, which is never a valid handle. Using `x` again as a handle traps (§ 10.7): the Component Model rejects the index `0`. Assigning `x` a new handle makes it usable again.

**Borrows.** Calling a method on `x` (`x.add(…)`), passing `x` for a parameter of type `borrow<T>`, or giving it to `stream::write` or `stream::read`, **borrows** it: the handle stays in `x`. A parameter of type `borrow<T>` can only be borrowed again: moving it (returning it, passing it for a `T` parameter, assigning it) is an error (E0219).

**Release.** A handle is given back to its owner (`resource.drop`, § 12.10) when the place that owns it gives it up without moving it:

- a local, at the end of the block that declares it, including when `break`, `continue`, `return` or `?` leave the block, and when a new value is assigned to it (the old handle is released after the new value is computed);
- a parameter of type `T`, on every exit of the function;
- a temporary handle that is borrowed (`carts::cart().total()`, `f(carts::cart())` with a `borrow` parameter), right after the call that borrows it;
- a temporary given to `_ =`, the payload of a `match` case written without a name, and an element of a destructuring written `_`, with every handle they contain;
- the operand of `?` gives its payload to the expression, and its error to the `return`: nothing is released.

The empty handle `0` is never released. A handle that was moved is therefore released once, by its new owner:

```wasl
func checkout(items: list<item>) -> result<receipt, carts::cart_error> {
  local c: carts::cart = carts::cart();           // `c` owns a new handle
  local i: u32;
  while (i < list::len(items)) {
    c.add(items[i].sku, items[i].quantity)?;      // borrows `c`; on error, releases `c` and returns
    i = i + 1;
  }
  return result::ok(receipt { items: c.count(), total: c.total() });   // releases `c`
}
```

**Streams and futures.** Their ends follow the same rules (§ 7.13; *specified, not available yet*): releasing a `stream_writer<T>` closes its stream, releasing a readable end tells the writer that nobody reads any more, and releasing a `future_writer<T>` that has not written its value traps (§ 10.7).

**Received from outside.** An exported function owns the handles it receives as `T` and releases them as above. The handles it receives as `borrow<T>` belong to its caller, and the Component Model requires them to be released before the call returns: the adapter of the function does it (§ 13.8).

**Resources of the package** are not handles in the components where their interface is compiled: they are WasmGC references, shared and reclaimed by the runtime (§ 10.8). The rules of this section still apply to their code outside their interface when a world imports that interface, so that a program is valid or invalid for every world at once (§ 7.12).
