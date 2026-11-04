//! Handles (specification, § 7.12, § 9.9, § 12.10): resources of another component,
//! provided here by the host, which counts the handles it gets back.

use std::collections::HashSet;

use wasmtime::component::{Component, Instance, Linker, Resource, ResourceAny, ResourceType, Val};
use wasmtime::{Engine, Store, StoreContextMut};

use waslc::{Dependencies, Package, SourceMap, diag::Code};

const SHOP: &str = r#"
package demo:shop@1.0.0;

interface carts {
  enum cart-error { unknown-sku, zero-quantity }

  resource cart {
    constructor();
    add: func(sku: string, quantity: u32) -> result<_, cart-error>;
    total: func() -> u64;
    of: static func(sku: string, quantity: u32) -> result<cart, cart-error>;
  }

  find: func(id: u32) -> option<cart>;
  merge: func(a: cart, b: borrow<cart>) -> cart;
}
"#;

/// The carts of the host: the total of each, by rep, and the reps it got back.
#[derive(Default)]
struct Shop {
    totals: Vec<u64>,
    dropped: Vec<u32>,
}

impl Shop {
    fn live(&self) -> usize {
        let dropped: HashSet<u32> = self.dropped.iter().copied().collect();
        assert_eq!(dropped.len(), self.dropped.len(), "a cart released twice");
        self.totals.len() - dropped.len()
    }
}

struct Cart;

fn new_cart(store: &mut StoreContextMut<'_, Shop>, total: u64) -> Val {
    let rep = store.data().totals.len() as u32;
    store.data_mut().totals.push(total);
    let r = Resource::<Cart>::new_own(rep);
    Val::Resource(r.try_into_resource_any(store).unwrap())
}

fn rep(store: &mut StoreContextMut<'_, Shop>, v: &Val) -> u32 {
    let Val::Resource(r) = v else {
        panic!("not a cart: {v:?}")
    };
    let r: Resource<Cart> = Resource::try_from_resource_any(*r, store).unwrap();
    r.rep()
}

fn price(sku: &str) -> Option<u64> {
    match sku {
        "pen" => Some(150),
        "notebook" => Some(425),
        _ => None,
    }
}

fn error(case: &str) -> Val {
    Val::Result(Err(Some(Box::new(Val::Enum(case.into())))))
}

fn linker(engine: &Engine) -> Linker<Shop> {
    let mut linker: Linker<Shop> = Linker::new(engine);
    let mut carts = linker.instance("demo:shop/carts@1.0.0").unwrap();
    carts
        .resource("cart", ResourceType::host::<Cart>(), |mut store, rep| {
            store.data_mut().dropped.push(rep);
            Ok(())
        })
        .unwrap();
    carts
        .func_new("[constructor]cart", |mut store, _, _, out| {
            out[0] = new_cart(&mut store, 0);
            Ok(())
        })
        .unwrap();
    carts
        .func_new("[method]cart.add", |mut store, _, args, out| {
            let r = rep(&mut store, &args[0]);
            let (Val::String(sku), Val::U32(n)) = (args[1].clone(), args[2].clone()) else {
                panic!()
            };
            out[0] = match price(&sku) {
                _ if n == 0 => error("zero-quantity"),
                None => error("unknown-sku"),
                Some(p) => {
                    store.data_mut().totals[r as usize] += p * n as u64;
                    Val::Result(Ok(None))
                }
            };
            Ok(())
        })
        .unwrap();
    carts
        .func_new("[method]cart.total", |mut store, _, args, out| {
            let r = rep(&mut store, &args[0]);
            out[0] = Val::U64(store.data().totals[r as usize]);
            Ok(())
        })
        .unwrap();
    carts
        .func_new("[static]cart.of", |mut store, _, args, out| {
            let (Val::String(sku), Val::U32(n)) = (args[0].clone(), args[1].clone()) else {
                panic!()
            };
            out[0] = match price(&sku) {
                None => error("unknown-sku"),
                Some(p) => Val::Result(Ok(Some(Box::new(new_cart(&mut store, p * n as u64))))),
            };
            Ok(())
        })
        .unwrap();
    carts
        .func_new("find", |mut store, _, args, out| {
            let Val::U32(id) = args[0] else { panic!() };
            out[0] = Val::Option((id > 0).then(|| Box::new(new_cart(&mut store, id as u64))));
            Ok(())
        })
        .unwrap();
    carts
        .func_new("merge", |mut store, _, args, out| {
            let (a, b) = (rep(&mut store, &args[0]), rep(&mut store, &args[1]));
            let total = store.data().totals[a as usize] + store.data().totals[b as usize];
            // The host owns `a` now, and gives it back.
            store.data_mut().dropped.push(a);
            out[0] = new_cart(&mut store, total);
            Ok(())
        })
        .unwrap();
    linker
}

fn deps() -> Dependencies {
    let mut deps = Dependencies::default();
    deps.add_wit("demo:shop", "shop.wit", SHOP).unwrap();
    deps
}

fn analyze(source: &str) -> waslc::Result<waslc::Analysis> {
    let package = Package::new("test", "app", "0.1.0").unwrap();
    let mut sources = SourceMap::default();
    sources.add("src/main.wasl".into(), source.to_string());
    waslc::analyze(&package, &sources, &deps())
}

fn fails(source: &str, code: Code, message: &str) {
    match analyze(source) {
        Ok(_) => panic!("accepted: {source}"),
        Err(e) => {
            assert_eq!(e.code, code, "{}", e.message);
            assert!(e.message.contains(message), "{}", e.message);
        }
    }
}

struct Runner {
    store: Store<Shop>,
    instance: Instance,
}

impl Runner {
    fn new(source: &str) -> Self {
        let mut sources = SourceMap::default();
        sources.add("src/main.wasl".into(), source.to_string());
        let bytes = analyze(source)
            .and_then(|analysis| analysis.build("w"))
            .unwrap_or_else(|e| panic!("{}", e.display(&sources)));
        let engine = Engine::default();
        let component = Component::new(&engine, &bytes).unwrap();
        let mut store = Store::new(&engine, Shop::default());
        let instance = linker(&engine).instantiate(&mut store, &component).unwrap();
        Runner { store, instance }
    }

    fn call(&mut self, name: &str, args: &[Val]) -> Option<Val> {
        let iface = self
            .instance
            .get_export_index(&mut self.store, None, "test:app/t@0.1.0")
            .unwrap();
        let index = self
            .instance
            .get_export_index(&mut self.store, Some(&iface), name)
            .unwrap_or_else(|| panic!("function `{name}`"));
        let func = self.instance.get_func(&mut self.store, index).unwrap();
        let mut results = vec![Val::Bool(false); func.ty(&self.store).results().len()];
        func.call(&mut self.store, args, &mut results)
            .unwrap_or_else(|e| panic!("{name}{args:?}: {e:?}"));
        results.pop()
    }

    /// Calls `name`, then checks that every cart it created was given back.
    fn released(&mut self, name: &str, args: &[Val]) -> Option<Val> {
        let result = self.call(name, args);
        assert_eq!(self.store.data().live(), 0, "`{name}` kept a cart");
        result
    }

    fn cart(&mut self, total: u64) -> ResourceAny {
        let mut store = self.store.as_context_mut();
        let Val::Resource(r) = new_cart(&mut store, total) else {
            unreachable!()
        };
        r
    }
}

use wasmtime::AsContextMut;

const PROGRAM: &str = r#"
world w {
  import demo:shop/carts;
  export t;
}

interface t {
  func basket(n: u32) -> u64 {
    local c: carts::cart = carts::cart();
    _ = c.add("pen", n);
    return c.total();
  }

  func checked(sku: string) -> result<u64, carts::cart_error> {
    local c: carts::cart = carts::cart();
    c.add(sku, 1)?;
    return result::ok(c.total());
  }

  func merged() -> result<u64, carts::cart_error> {
    local a: carts::cart = carts::cart::of("pen", 2)?;
    local b: carts::cart = carts::cart::of("notebook", 1)?;
    local m: carts::cart = carts::merge(a, b);
    return result::ok(m.total() + b.total());
  }

  func temporaries() -> u64 {
    _ = carts::cart::of("pen", 1);
    _ = carts::find(3);
    return carts::cart().total() + carts::merge(carts::cart(), carts::cart()).total();
  }

  func lookup(id: u32) -> u64 {
    match (carts::find(id)) {
      case some(c) {
        return c.total();
      }
      case none {
        return 0;
      }
    }
  }

  func unnamed(sku: string) -> bool {
    match (carts::cart::of(sku, 1)) {
      case ok {
        return true;
      }
      case err {
        return false;
      }
    }
  }

  func loops(n: u32) -> u32 {
    local c: carts::cart = carts::cart();
    local i: u32;
    while (i < n) {
      c = carts::cart();
      local d: carts::cart = carts::cart();
      if (i == 2) {
        break;
      }
      i = i + 1;
      if (i == 1) {
        continue;
      }
      local e: carts::cart = carts::cart();
    }
    return i;
  }

  func pass(a: carts::cart, b: borrow<carts::cart>) -> carts::cart {
    _ = a.add("pen", 1);
    return carts::merge(a, b);
  }

  func keep(a: carts::cart) -> u64 {
    return a.total();
  }

  func give(n: u32) -> option<carts::cart> {
    if (n == 0) {
      return option::none;
    }
    local c: carts::cart = carts::cart();
    _ = c.add("pen", n);
    return option::some(c);
  }
}
"#;

#[test]
fn handles_are_released() {
    let mut r = Runner::new(PROGRAM);
    assert_eq!(r.released("basket", &[Val::U32(2)]), Some(Val::U64(300)));
    assert_eq!(
        r.released("checked", &[Val::String("pen".into())]),
        Some(Val::Result(Ok(Some(Box::new(Val::U64(150))))))
    );
    // The early return of `?` releases the cart too.
    assert_eq!(
        r.released("checked", &[Val::String("eraser".into())]),
        Some(error("unknown-sku"))
    );
    // `a` is moved to `merge`, `b` is lent to it.
    assert_eq!(
        r.released("merged", &[]),
        Some(Val::Result(Ok(Some(Box::new(Val::U64(300 + 425 + 425))))))
    );
    assert_eq!(r.released("temporaries", &[]), Some(Val::U64(0)));
    assert_eq!(r.released("lookup", &[Val::U32(7)]), Some(Val::U64(7)));
    assert_eq!(r.released("lookup", &[Val::U32(0)]), Some(Val::U64(0)));
    assert_eq!(
        r.released("unnamed", &[Val::String("pen".into())]),
        Some(Val::Bool(true))
    );
    assert_eq!(
        r.released("unnamed", &[Val::String("x".into())]),
        Some(Val::Bool(false))
    );
    assert_eq!(r.released("loops", &[Val::U32(5)]), Some(Val::U32(2)));
    assert_eq!(r.released("loops", &[Val::U32(0)]), Some(Val::U32(0)));
}

#[test]
fn handles_across_exports() {
    let mut r = Runner::new(PROGRAM);
    // An owned handle is released by the function; a lent one stays with the caller.
    let a = r.cart(1);
    assert_eq!(r.call("keep", &[Val::Resource(a)]), Some(Val::U64(1)));
    assert_eq!(r.store.data().live(), 0);

    let (a, b) = (r.cart(10), r.cart(20));
    let Some(Val::Resource(m)) = r.call("pass", &[Val::Resource(a), Val::Resource(b)]) else {
        panic!("no cart")
    };
    // `a` went to `merge`; `b` and the new cart are the caller's.
    assert_eq!(r.store.data().live(), 2);
    let mut store = r.store.as_context_mut();
    let m: Resource<Cart> = Resource::try_from_resource_any(m, &mut store).unwrap();
    assert_eq!(r.store.data().totals[m.rep() as usize], 10 + 150 + 20);

    let Some(Val::Option(Some(c))) = r.call("give", &[Val::U32(2)]) else {
        panic!("no cart")
    };
    assert!(matches!(*c, Val::Resource(_)));
    assert_eq!(r.store.data().live(), 3);
    assert_eq!(r.call("give", &[Val::U32(0)]), Some(Val::Option(None)));
}

#[test]
fn handle_positions() {
    let wrap = |items: &str| {
        format!("world w {{ import demo:shop/carts; export t; }}\ninterface t {{\n{items}\n}}")
    };
    fails(
        &wrap("record r { c: carts::cart }"),
        Code::HandlePosition,
        "the field of a record cannot hold one",
    );
    fails(
        &wrap("func f() -> list<carts::cart> { return []; }"),
        Code::HandlePosition,
        "a list cannot hold one",
    );
    fails(
        &wrap("func f() { local o: option<carts::cart> = carts::find(1); }"),
        Code::HandlePosition,
        "a local can only hold a handle itself",
    );
    fails(
        &wrap("func f(o: option<carts::cart>) { }"),
        Code::HandlePosition,
        "a local can only hold a handle itself",
    );
    fails(
        &wrap("resource s { c: carts::cart, constructor() { return s { c: carts::cart() }; } }"),
        Code::HandlePosition,
        "the field of a resource cannot hold one",
    );
    // A resource of the package is a handle type when a world imports its interface.
    fails(
        "world a { export shop; }
         world b { import shop; export t; }
         interface shop { resource cart { constructor() { return cart {}; } } }
         interface t { func f() -> tuple<shop::cart, u32> { return tuple(shop::cart(), 1); } }",
        Code::HandlePosition,
        "a tuple cannot hold one",
    );
    // A lent handle cannot be given away.
    fails(
        &wrap("func f(b: borrow<carts::cart>) -> carts::cart { return b; }"),
        Code::BorrowMisuse,
        "only lent to the function",
    );
}

/// A resource of the package used by a world that imports its interface, and by a world
/// that exports it: a handle in one, a struct in the other.
#[test]
fn package_resources_in_both_worlds() {
    let source = "
world shop { export carts; }
world w { import carts; export t; }

interface carts {
  resource cart {
    total: mut u64,
    constructor() { return cart { total: 0 }; }
    func add(n: u64) { self.total = self.total + n; }
    func get() -> u64 { return self.total; }
  }
}

interface t {
  func run(n: u64) -> u64 {
    local c: carts::cart = carts::cart();
    c.add(n);
    c.add(1);
    return c.get();
  }
}
";
    let analysis = analyze(source).unwrap_or_else(|e| panic!("{}", e.message));
    let shop = analysis.build("shop").unwrap();
    let w = analysis.build("w").unwrap();
    for bytes in [&shop, &w] {
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(bytes)
            .unwrap();
    }
}
