//! Structural equality (specification, § 8.3): `==` and `!=` on records, tuples, lists,
//! variants, options and results, run by wasmtime, and their errors.

use std::sync::OnceLock;

use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Engine, Store};

use waslc::diag::Code;
use waslc::{Package, SourceMap};

const INTERFACE: &str = "test:eq/t@0.1.0";

const PROGRAM: &str = r#"
world w {
  export t;
}

interface t {
  record point { x: s32, y: s32 }
  record line { from: point, to: point, label: option<string> }
  variant shape { dot(point), path(list<point>), empty }
  enum color { red, green }
  flags style { bold, italic }
  record measure { value: f64 }

  func points(a: s32, b: s32) -> bool {
    return point { x: a, y: 1 } == point { x: b, y: 1 };
  }
  func lines(same_label: bool) -> bool {
    local p = point { x: 1, y: 2 };
    local l1 = line { from: p, to: p, label: option.some("a") };
    local l2 = line { from: point { x: 1, y: 2 }, to: p, label: option.some("a") };
    if (!same_label)
      l2 = line { from: p, to: p, label: option.none };
    return l1 == l2;
  }
  func differ(a: list<point>, b: list<point>) -> bool { return a != b; }
  func lists(a: list<u8>, b: list<u8>) -> bool { return a == b; }
  func nested(a: list<list<string>>, b: list<list<string>>) -> bool { return a == b; }
  func tuples(n: u64) -> bool { return tuple(n, "x", 'c') == tuple(7, "x", 'c'); }
  func shapes(k: u32) -> bool {
    local a: shape = shape::path([point { x: 1, y: 1 }]);
    local b = shape::empty;
    if (k == 1) b = shape::path([point { x: 1, y: 1 }]);
    if (k == 2) b = shape::path([point { x: 1, y: 2 }]);
    if (k == 3) b = shape::dot(point { x: 1, y: 1 });
    return a == b;
  }
  func options(a: option<u32>, b: option<u32>) -> bool { return a == b; }
  func nested_options(a: option<option<u32>>, b: option<option<u32>>) -> bool { return a == b; }
  func results(a: result<u32, string>, b: result<u32, string>) -> bool { return a == b; }
  func empty_results(a: result, b: result) -> bool { return a == b; }
  func scalars(c: color, s: style) -> bool {
    return tuple(c, s) == tuple(color::green, style { bold });
  }
  func recursive(n: u32) -> bool {
    local a = trees::expr::add(tuple(trees::expr::lit(1), trees::expr::neg(trees::expr::lit(2))));
    local b = trees::expr::add(tuple(trees::expr::lit(1), trees::expr::neg(trees::expr::lit(n))));
    return a == b;
  }
  func nan() -> bool {
    local m = measure { value: 0.0 / 0.0 };
    return m == m;
  }
}

// Recursive types stay inside the component (E0208).
interface trees {
  variant expr { lit(u32), neg(expr), add(tuple<expr, expr>) }
}
"#;

fn package() -> Package {
    Package::new("test", "eq", "0.1.0").unwrap()
}

fn sources(program: &str) -> SourceMap {
    let mut sources = SourceMap::default();
    sources.add("src/t.wasl".into(), program.to_string());
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

fn call(name: &str, args: &[Val]) -> bool {
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
    match results[0] {
        Val::Bool(b) => b,
        ref other => panic!("{name}: {other:?}"),
    }
}

fn point(x: i32, y: i32) -> Val {
    Val::Record(vec![("x".into(), Val::S32(x)), ("y".into(), Val::S32(y))])
}

fn list(items: Vec<Val>) -> Val {
    Val::List(items)
}

fn bytes(b: &[u8]) -> Val {
    list(b.iter().map(|&x| Val::U8(x)).collect())
}

fn strings(rows: &[&[&str]]) -> Val {
    list(
        rows.iter()
            .map(|r| list(r.iter().map(|s| Val::String((*s).into())).collect()))
            .collect(),
    )
}

fn opt(v: Option<Val>) -> Val {
    Val::Option(v.map(Box::new))
}

#[test]
fn records_and_tuples() {
    assert!(call("points", &[Val::S32(3), Val::S32(3)]));
    assert!(!call("points", &[Val::S32(3), Val::S32(4)]));
    assert!(call("lines", &[Val::Bool(true)]));
    assert!(!call("lines", &[Val::Bool(false)]));
    assert!(call("tuples", &[Val::U64(7)]));
    assert!(!call("tuples", &[Val::U64(8)]));
    assert!(call(
        "scalars",
        &[Val::Enum("green".into()), Val::Flags(vec!["bold".into()])]
    ));
    assert!(!call(
        "scalars",
        &[Val::Enum("green".into()), Val::Flags(vec![])]
    ));
    // A NaN is not equal to itself, inside a record too.
    assert!(!call("nan", &[]));
}

#[test]
fn lists() {
    assert!(call("lists", &[bytes(&[1, 200]), bytes(&[1, 200])]));
    assert!(!call("lists", &[bytes(&[1, 200]), bytes(&[1, 201])]));
    assert!(!call("lists", &[bytes(&[1]), bytes(&[1, 2])]));
    assert!(call("lists", &[bytes(&[]), bytes(&[])]));
    let a = list(vec![point(1, 2), point(3, 4)]);
    assert!(!call("differ", &[a.clone(), a.clone()]));
    assert!(call("differ", &[a, list(vec![point(1, 2), point(3, 5)])]));
    assert!(call(
        "nested",
        &[strings(&[&["a", "b"], &[]]), strings(&[&["a", "b"], &[]])]
    ));
    assert!(!call(
        "nested",
        &[strings(&[&["a", "b"]]), strings(&[&["a", "c"]])]
    ));
}

#[test]
fn variants_options_and_results() {
    assert!(call("shapes", &[Val::U32(1)]));
    assert!(!call("shapes", &[Val::U32(0)]));
    assert!(!call("shapes", &[Val::U32(2)]));
    assert!(!call("shapes", &[Val::U32(3)]));
    assert!(call("recursive", &[Val::U32(2)]));
    assert!(!call("recursive", &[Val::U32(3)]));

    assert!(call(
        "options",
        &[opt(Some(Val::U32(1))), opt(Some(Val::U32(1)))]
    ));
    assert!(!call(
        "options",
        &[opt(Some(Val::U32(1))), opt(Some(Val::U32(2)))]
    ));
    assert!(!call("options", &[opt(Some(Val::U32(1))), opt(None)]));
    assert!(!call("options", &[opt(None), opt(Some(Val::U32(1)))]));
    assert!(call("options", &[opt(None), opt(None)]));
    let some_none = opt(Some(opt(None)));
    assert!(call(
        "nested-options",
        &[some_none.clone(), some_none.clone()]
    ));
    assert!(!call("nested-options", &[some_none, opt(None)]));

    let ok = |n| Val::Result(Ok(Some(Box::new(Val::U32(n)))));
    let error = |s: &str| Val::Result(Err(Some(Box::new(Val::String(s.into())))));
    assert!(call("results", &[ok(1), ok(1)]));
    assert!(!call("results", &[ok(1), ok(2)]));
    assert!(!call("results", &[ok(1), error("x")]));
    assert!(call("results", &[error("x"), error("x")]));
    assert!(!call("results", &[error("x"), error("y")]));
    assert!(call(
        "empty-results",
        &[Val::Result(Ok(None)), Val::Result(Ok(None))]
    ));
    assert!(!call(
        "empty-results",
        &[Val::Result(Ok(None)), Val::Result(Err(None))]
    ));
}

#[test]
fn errors() {
    let cases: &[(&str, &str)] = &[
        (
            "record m { x: mut u32 }",
            "func f(a: m) -> bool { return a == a; }",
        ),
        ("", "func f(a: list<mut u8>) -> bool { return a == a; }"),
        (
            "record m { x: mut u32 }",
            "func f(a: list<option<m>>) -> bool { return a == a; }",
        ),
        (
            "resource r { constructor() { return r { }; } }",
            "func f(a: r) -> bool { return a == a; }",
        ),
        ("", "func f(a: stream<u8>) -> bool { return a == a; }"),
    ];
    for (decls, func) in cases {
        let program = format!(
            "world w {{ export t; }}\ninterface t {{ func g() {{ }} }}\ninterface x {{\n  {decls}\n  {func}\n}}\n"
        );
        let sources = sources(&program);
        let error = match waslc::analyze(&package(), &sources, &Default::default()) {
            Ok(_) => panic!("`{func}` is accepted"),
            Err(mut errors) => errors.remove(0),
        };
        let shown = error.display(&sources).to_string();
        assert_eq!(error.code, Code::InvalidOperator, "`{func}`: {shown}");
        assert!(shown.contains("has no equality"), "`{func}`: {shown}");
    }
}
