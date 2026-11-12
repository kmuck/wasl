//! Resources of the package (specification, § 7.12, § 8.20, § 12.10 and § 13.11):
//! WasmGC objects inside the component, handles for the components that use them.

use wasmtime::component::{Component, Instance, Linker, ResourceAny, Val};
use wasmtime::{Engine, Store};

use waslc::{Dependencies, Package, SourceMap, diag::Code};

const CATALOG: &str = include_str!("../../examples/catalog/src/catalog.wasl");

fn analyze(source: &str) -> waslc::Result<waslc::Analysis> {
    let package = Package::new("example", "catalog", "0.1.0").unwrap();
    let mut sources = SourceMap::default();
    sources.add("src/catalog.wasl".into(), source.to_string());
    waslc::analyze(&package, &sources, &Dependencies::default()).map_err(|mut e| e.remove(0))
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
    store: Store<()>,
    instance: Instance,
}

impl Runner {
    fn new(source: &str, world: &str) -> Self {
        let mut sources = SourceMap::default();
        sources.add("src/catalog.wasl".into(), source.to_string());
        let bytes = analyze(source)
            .and_then(|analysis| analysis.build(world))
            .unwrap_or_else(|e| panic!("{}", e.display(&sources)));
        let engine = Engine::default();
        let component = Component::new(&engine, &bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let linker: Linker<()> = Linker::new(&engine);
        let instance = linker.instantiate(&mut store, &component).unwrap();
        Runner { store, instance }
    }

    fn call(&mut self, name: &str, args: &[Val]) -> Option<Val> {
        let iface = self
            .instance
            .get_export_index(&mut self.store, None, "example:catalog/carts@0.1.0")
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

    fn cart(&mut self) -> ResourceAny {
        match self.call("[constructor]cart", &[]) {
            Some(Val::Resource(r)) => r,
            other => panic!("not a cart: {other:?}"),
        }
    }

    fn add(&mut self, cart: ResourceAny, sku: &str, quantity: u32) -> Val {
        self.call(
            "[method]cart.add",
            &[
                Val::Resource(cart),
                Val::String(sku.into()),
                Val::U32(quantity),
            ],
        )
        .unwrap()
    }

    fn total(&mut self, cart: ResourceAny) -> u64 {
        match self.call("[method]cart.total", &[Val::Resource(cart)]) {
            Some(Val::U64(n)) => n,
            other => panic!("not a total: {other:?}"),
        }
    }
}

fn ok() -> Val {
    Val::Result(Ok(None))
}

fn err(case: &str) -> Val {
    Val::Result(Err(Some(Box::new(Val::Enum(case.into())))))
}

#[test]
fn catalog() {
    let mut r = Runner::new(CATALOG, "catalog");
    let c = r.cart();
    assert_eq!(r.add(c, "pen", 2), ok());
    assert_eq!(r.add(c, "pen", 0), err("zero-quantity"));
    assert_eq!(r.add(c, "eraser", 1), err("unknown-sku"));
    assert_eq!(r.add(c, "notebook", 1), ok());
    assert_eq!(r.total(c), 2 * 150 + 425);
    assert_eq!(
        r.call("[method]cart.count", &[Val::Resource(c)]),
        Some(Val::U32(3))
    );
    assert_eq!(
        r.call("[method]cart.last-line", &[Val::Resource(c)]),
        Some(Val::Option(Some(Box::new(Val::Record(vec![
            ("sku".into(), Val::String("notebook".into())),
            ("quantity".into(), Val::U32(1)),
        ])))))
    );

    let of = |r: &mut Runner, sku: &str, n: u32| {
        r.call("[static]cart.of", &[Val::String(sku.into()), Val::U32(n)])
            .unwrap()
    };
    let Val::Result(Ok(Some(single))) = of(&mut r, "notebook", 2) else {
        panic!("no cart")
    };
    let Val::Resource(single) = *single else {
        panic!("not a cart")
    };
    assert_eq!(r.total(single), 850);
    assert_eq!(of(&mut r, "eraser", 1), err("unknown-sku"));
    // The first cart is unchanged.
    assert_eq!(r.total(c), 725);
}

#[test]
fn handles_are_released_and_reused() {
    let mut r = Runner::new(CATALOG, "catalog");
    // More carts than the first table holds, each with its own total.
    let carts: Vec<ResourceAny> = (0..20).map(|_| r.cart()).collect();
    for (k, &c) in carts.iter().enumerate() {
        assert_eq!(r.add(c, "pen", k as u32 + 1), ok());
    }
    for (k, &c) in carts.iter().enumerate() {
        assert_eq!(r.total(c), 150 * (k as u64 + 1));
    }
    // Dropping a handle runs the destructor, which frees the slot for the next cart.
    for &c in &carts[..10] {
        c.resource_drop(&mut r.store).unwrap();
    }
    let more: Vec<ResourceAny> = (0..15).map(|_| r.cart()).collect();
    for &c in &more {
        assert_eq!(r.add(c, "notebook", 1), ok());
        assert_eq!(r.total(c), 425);
    }
    for (k, &c) in carts.iter().enumerate().skip(10) {
        assert_eq!(r.total(c), 150 * (k as u64 + 1));
    }
}

#[test]
fn own_handles_as_parameters() {
    // A function that takes a cart takes it over: the handle is consumed.
    let source = CATALOG.replace(
        "  resource cart {",
        "  func merge(a: cart, b: cart) -> cart {
    a.add(\"pen\", b.count());
    return a;
  }

  func merged_total(a: cart, b: borrow<cart>) -> u64 {
    return a.total() + b.total();
  }

  resource cart {",
    );
    let mut r = Runner::new(&source, "catalog");
    let a = r.cart();
    let b = r.cart();
    assert_eq!(r.add(b, "notebook", 3), ok());
    let Some(Val::Resource(m)) = r.call("merge", &[Val::Resource(a), Val::Resource(b)]) else {
        panic!("no cart")
    };
    assert_eq!(r.total(m), 3 * 150);
    let c = r.cart();
    assert_eq!(r.add(c, "pen", 1), ok());
    assert_eq!(
        r.call("merged-total", &[Val::Resource(m), Val::Resource(c)]),
        Some(Val::U64(600))
    );
    // `c` was only borrowed.
    assert_eq!(r.total(c), 150);
}

#[test]
fn errors() {
    let wrap = |items: &str| format!("world w {{ export r; }}\ninterface r {{\n{items}\n}}");
    // A borrow is only a parameter.
    fails(
        &wrap(
            "resource c { constructor() { return c {}; } func me() -> borrow<c> { return self; } }",
        ),
        Code::BorrowMisuse,
        "only allowed as the type of a parameter",
    );
    fails(
        &wrap("record x { a: u32 }\nfunc f(b: borrow<x>) {}"),
        Code::BorrowMisuse,
        "only a resource can be borrowed",
    );
    fails(
        &wrap("resource c { constructor() { return c {}; } func me() -> c { return self; } }"),
        Code::BorrowMisuse,
        "only lent to the function",
    );
    // Methods.
    fails(
        &wrap(
            "resource c { constructor() { return c {}; } func n() -> u32 { return 1; } }\nfunc f(x: c) -> u32 { return x.size(); }",
        ),
        Code::UnknownMethod,
        "has no method `size`",
    );
    fails(
        &wrap("func f(x: u32) -> u32 { return x.size(); }"),
        Code::UnknownMethod,
        "only resources have methods",
    );
    fails(
        &wrap("resource c { func n() -> u32 { return 1; } }\nfunc f() -> c { return c(); }"),
        Code::UnknownMethod,
        "has no constructor",
    );
    // Fields are private to the interface of the resource.
    fails(
        "world w { export r; }
interface r {
  resource c { n: u32, constructor() { return c { n: 1 }; } }
}
interface s {
  func f(x: r::c) -> u32 { return x.n; }
}",
        Code::PrivateField,
        "private to interface `r`",
    );
    // Members are unique.
    fails(
        &wrap("resource c { n: u32, func n() -> u32 { return 1; } }"),
        Code::DuplicateFunc,
        "`n`",
    );
}
