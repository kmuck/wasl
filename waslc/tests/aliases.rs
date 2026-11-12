//! Type aliases (specification, § 7.14): the same type under another name, run by
//! wasmtime, their WIT, and their errors.

use std::sync::OnceLock;

use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Engine, Store};

use waslc::diag::Code;
use waslc::{Package, SourceMap};

const INTERFACE: &str = "test:alias/shop@0.1.0";

const PROGRAM: &str = r#"
world w {
  export shop;
}

interface money {
  type cents = u64;
}

interface shop {
  type price = money::cents;                 // an alias of an alias
  type lines = list<order_line>;             // used before the record
  type sku = string;
  type line = order_line;                    // an alias of a declared type
  type maybe_price = option<price>;

  record order_line { sku: sku, quantity: u32, unit_price: price }
  enum state { open, closed }
  type st = state;

  func total(lines: lines) -> price {
    local total: price = 0;
    for (l in lines)
      total = total + u64(l.quantity) * l.unit_price;
    return total;
  }

  // An alias is the type it names: no conversion in either direction.
  func mixed(a: u64, b: price) -> u64 {
    local c: money::cents = a + b;
    return c;
  }

  // A literal and a case through the alias of a declared type.
  func one(s: sku) -> lines {
    local l = line { sku: s, quantity: 1, unit_price: 250 };
    return [l];
  }
  func closed() -> st { return st::closed; }
  func cheapest(lines: lines) -> maybe_price {
    if (list.len(lines) == 0) return option.none;
    return option.some(lines[0].unit_price);
  }
}
"#;

fn package() -> Package {
    Package::new("test", "alias", "0.1.0").unwrap()
}

fn sources(program: &str) -> SourceMap {
    let mut sources = SourceMap::default();
    sources.add("src/shop.wasl".into(), program.to_string());
    sources
}

fn engine_and_component() -> &'static (Engine, Component) {
    static CELL: OnceLock<(Engine, Component)> = OnceLock::new();
    CELL.get_or_init(|| {
        let sources = sources(PROGRAM);
        let bytes = waslc::analyze(&package(), &sources, &Default::default())
            .map_err(|mut e| e.remove(0))
            .and_then(|analysis| analysis.build("w"))
            .unwrap_or_else(|e| panic!("{}", e.display(&sources)));
        let engine = Engine::default();
        let component = Component::new(&engine, &bytes).unwrap();
        (engine, component)
    })
}

/// Calls a function on a fresh instance.
fn call(name: &str, args: &[Val]) -> Val {
    let (engine, component) = engine_and_component();
    let mut store = Store::new(engine, ());
    let instance = Linker::new(engine)
        .instantiate(&mut store, component)
        .unwrap();
    let iface = instance
        .get_export_index(&mut store, None, INTERFACE)
        .expect("exported interface");
    let index = instance
        .get_export_index(&mut store, Some(&iface), name)
        .unwrap_or_else(|| panic!("exported function `{name}`"));
    let func = instance.get_func(&mut store, index).unwrap();
    let mut results = [Val::Bool(false)];
    func.call(&mut store, args, &mut results)
        .unwrap_or_else(|e| panic!("{name}{args:?}: {e:?}"));
    results[0].clone()
}

fn line(sku: &str, quantity: u32, unit_price: u64) -> Val {
    Val::Record(vec![
        ("sku".into(), Val::String(sku.into())),
        ("quantity".into(), Val::U32(quantity)),
        ("unit-price".into(), Val::U64(unit_price)),
    ])
}

#[test]
fn aliases_are_the_types_they_name() {
    let lines = Val::List(vec![line("a", 2, 100), line("b", 1, 50)]);
    assert_eq!(call("total", std::slice::from_ref(&lines)), Val::U64(250));
    assert_eq!(call("mixed", &[Val::U64(1), Val::U64(2)]), Val::U64(3));
    assert_eq!(
        call("one", &[Val::String("x".into())]),
        Val::List(vec![line("x", 1, 250)])
    );
    assert_eq!(call("closed", &[]), Val::Enum("closed".into()));
    assert_eq!(
        call("cheapest", &[lines]),
        Val::Option(Some(Box::new(Val::U64(100))))
    );
    assert_eq!(call("cheapest", &[Val::List(vec![])]), Val::Option(None));
}

/// The WIT declares the aliases and keeps their names (§ 7.14, § 13.1).
#[test]
fn wit_keeps_the_names() {
    let sources = sources(PROGRAM);
    let analysis = waslc::analyze(&package(), &sources, &Default::default())
        .unwrap_or_else(|mut e| panic!("{}", e.remove(0).display(&sources)));
    let wit = analysis.wit().unwrap();
    for expected in [
        "interface money {\n  type cents = u64;\n}",
        "use money.{cents};",
        "type price = cents;",
        "type lines = list<order-line>;",
        "type line = order-line;",
        "type maybe-price = option<price>;",
        "sku: sku,",
        "unit-price: price,",
        "total: func(lines: lines) -> price;",
        "mixed: func(a: u64, b: price) -> u64;",
        "closed: func() -> st;",
        "cheapest: func(lines: lines) -> maybe-price;",
    ] {
        assert!(wit.contains(expected), "{expected}\n{wit}");
    }
}

#[test]
fn errors() {
    let cases: &[(&str, Code, &str)] = &[
        ("type a = a;", Code::AliasCycle, "`a` names itself"),
        ("type a = list<a>;", Code::AliasCycle, "`a` names itself"),
        (
            "type a = option<b>; type b = tuple<u32, a>;",
            Code::AliasCycle,
            "`a` names itself through `b`",
        ),
        (
            "type a = nothing;",
            Code::UnknownType,
            "unknown type `nothing`",
        ),
        (
            "type a = u32; type a = u64;",
            Code::DuplicateName,
            "already declared",
        ),
        (
            "type a = u32; record a { x: u32 }",
            Code::DuplicateName,
            "already declared",
        ),
        ("type f = u32;", Code::DuplicateName, "already declared"),
        ("type w = u32;", Code::TypeNamedLikeTopLevel, "world"),
        (
            "type a = list<borrow<r>>; resource r { }",
            Code::BorrowMisuse,
            "only allowed as the type of a parameter",
        ),
        (
            "type n = u32; func g() -> string { local k: n = \"x\"; return k; }",
            Code::TypeMismatch,
            "u32",
        ),
    ];
    for (items, code, message) in cases {
        let program =
            format!("world w {{ export t; }}\ninterface t {{\n  func f() {{ }}\n  {items}\n}}\n");
        let sources = sources(&program);
        let error = match waslc::analyze(&package(), &sources, &Default::default()) {
            Ok(_) => panic!("`{items}` is accepted"),
            Err(mut errors) => errors.remove(0),
        };
        let shown = error.display(&sources).to_string();
        assert_eq!(error.code, *code, "`{items}`: {shown}");
        assert!(shown.contains(message), "`{items}`: {shown}");
    }
}

/// An alias of an interface with functions that the world does not export cannot be
/// named in the WIT, like a declared type (E0209).
#[test]
fn alias_of_an_unexported_interface() {
    let program = "world w { export t; }
        interface other { type price = u64; func g() { } }
        interface t { func f(p: other::price) { } }";
    let map = sources(program);
    let error = match waslc::analyze(&package(), &map, &Default::default()) {
        Ok(_) => panic!("accepted"),
        Err(mut errors) => errors.remove(0),
    };
    assert_eq!(error.code, Code::UnexportedTypeInterface);
    // An alias of a type that cannot be in the WIT stays out of it.
    let program = "world w { export t; }
        interface t { type buffer = list<mut u8>; func f() -> u32 { local b: buffer = list.filled(2, 0); return list.len(b); } }";
    let map = sources(program);
    let analysis = waslc::analyze(&package(), &map, &Default::default())
        .unwrap_or_else(|mut e| panic!("{}", e.remove(0).display(&map)));
    let wit = analysis.wit().unwrap();
    assert!(!wit.contains("buffer"), "{wit}");
}
