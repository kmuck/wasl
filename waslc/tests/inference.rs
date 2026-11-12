//! Locals without type and `for` (specification, § 8.2, § 9.4, § 9.10), run by wasmtime,
//! and their errors.

use std::sync::OnceLock;

use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Engine, Store};

use waslc::diag::Code;
use waslc::{Package, SourceMap};

const INTERFACE: &str = "test:infer/t@0.1.0";

const PROGRAM: &str = r#"
world w {
  export t;
}

global seen: mut u32 = 0;

interface t {
  record line { quantity: u32, unit_price: u64 }

  // `local x = e;` takes the type of `e`; a literal alone is an `s32` or an `f64`
  func literal() -> s32 {
    local n = -7;
    n = n * 2;
    return n;
  }
  func float_literal() -> f64 {
    local r = 0.5;
    local twice = (r + 1.0) * 2.0;
    return twice;
  }
  func literal_expression() -> s32 {
    local n = (1 + 2) * 3;
    return n;
  }
  func from_operand(price: u64) -> u64 {
    local total = price * 2;
    return total;
  }
  func from_conversion(n: u32) -> u64 {
    local total = u64(n);
    total = total + 18446744073709551614;
    return total;
  }
  func from_call(s: string) -> u32 {
    local bytes = string::bytes(s);
    local n = list::len(bytes);
    return n;
  }
  func from_values(b: bool) -> string {
    local yes = "yes";
    local flag = !b;
    local some = option::some("no");
    if (flag) {
      return yes;
    }
    match (some) {
      case some(s) { return s; }
      case none { return ""; }
    }
  }
  func pair() -> tuple<u32, string> { return tuple(3, "abc"); }
  func destructured() -> u32 {
    local (n, s) = pair();
    local (m: u32, _) = pair();
    local (_, again) = pair();
    return n + m + string::len(s) + string::len(again);
  }

  // `for (x in xs)`
  func total(lines: list<line>) -> u64 {
    local total: u64 = 0;
    for (l in lines) {
      total = total + u64(l.quantity) * l.unit_price;
    }
    return total;
  }
  func evens(xs: list<u32>) -> u32 {
    local sum: u32;
    for (x in xs) {
      if (x % 2 == 1) {
        continue;
      }
      if (x == 100) {
        break;
      }
      sum = sum + x;
    }
    return sum;
  }
  func make() -> list<u32> {
    seen = seen + 1;
    return [1, 2, 3];
  }
  func evaluated_once() -> u32 {
    local sum: u32;
    for (x in make()) {
      sum = sum + x;
    }
    return sum * 10 + seen;
  }
  func nested(rows: list<list<u32>>) -> u32 {
    local sum: u32;
    for (row in rows) {
      for (x in row) {
        sum = sum + x;
      }
    }
    return sum;
  }
  func assigned(xs: list<u32>) -> u32 {
    local sum: u32;
    for (x in xs) {
      x = x + 1;
      sum = sum + x;
    }
    return sum + xs[0];
  }
  func mutable() -> u32 {
    local xs: list<mut u32> = list::filled(3, 1);
    local sum: u32;
    for (x in xs) {
      xs[2] = 10;
      sum = sum + x;
    }
    return sum;
  }
  func chars(s: string) -> u32 {
    local n: u32;
    for (c in string::chars(s)) {
      if (c == 'a') {
        n = n + 1;
      }
    }
    return n;
  }
  func find(xs: list<u32>, wanted: u32) -> bool {
    for (x in xs) {
      if (x == wanted) {
        return true;
      }
    }
    return false;
  }
  func siblings(xs: list<u32>) -> u32 {
    local n: u32;
    for (x in xs) {
      n = n + x;
    }
    for (x in xs) {
      n = n + x;
    }
    return n;
  }
}
"#;

fn package() -> Package {
    Package::new("test", "infer", "0.1.0").unwrap()
}

fn engine_and_component() -> &'static (Engine, Component) {
    static CELL: OnceLock<(Engine, Component)> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut sources = SourceMap::default();
        sources.add("src/t.wasl".into(), PROGRAM.to_string());
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

fn u32s(xs: &[u32]) -> Val {
    Val::List(xs.iter().map(|&x| Val::U32(x)).collect())
}

#[test]
fn locals_without_type() {
    assert_eq!(call("literal", &[]), Val::S32(-14));
    assert_eq!(call("float-literal", &[]), Val::Float64(3.0));
    assert_eq!(call("literal-expression", &[]), Val::S32(9));
    assert_eq!(
        call("from-operand", &[Val::U64(1 << 40)]),
        Val::U64(1 << 41)
    );
    assert_eq!(call("from-conversion", &[Val::U32(1)]), Val::U64(u64::MAX));
    assert_eq!(
        call("from-call", &[Val::String("héllo".into())]),
        Val::U32(6)
    );
    assert_eq!(
        call("from-values", &[Val::Bool(false)]),
        Val::String("yes".into())
    );
    assert_eq!(
        call("from-values", &[Val::Bool(true)]),
        Val::String("no".into())
    );
    assert_eq!(call("destructured", &[]), Val::U32(12));
}

#[test]
fn for_loops() {
    let line = |quantity: u32, unit_price: u64| {
        Val::Record(vec![
            ("quantity".into(), Val::U32(quantity)),
            ("unit-price".into(), Val::U64(unit_price)),
        ])
    };
    assert_eq!(
        call("total", &[Val::List(vec![line(3, 150), line(1, 900)])]),
        Val::U64(1350)
    );
    assert_eq!(call("total", &[Val::List(vec![])]), Val::U64(0));
    // `continue` goes to the next element, `break` leaves the loop.
    assert_eq!(call("evens", &[u32s(&[1, 2, 3, 4, 100, 6])]), Val::U32(6));
    // The list is evaluated once.
    assert_eq!(call("evaluated-once", &[]), Val::U32(61));
    assert_eq!(
        call(
            "nested",
            &[Val::List(vec![u32s(&[1, 2]), u32s(&[]), u32s(&[3])])]
        ),
        Val::U32(6)
    );
    // Assigning the element changes the local, not the list.
    assert_eq!(call("assigned", &[u32s(&[1, 2])]), Val::U32(6));
    // An element assigned during the loop is read with its new value.
    assert_eq!(call("mutable", &[]), Val::U32(12));
    assert_eq!(call("chars", &[Val::String("banana".into())]), Val::U32(3));
    assert_eq!(call("find", &[u32s(&[4, 5]), Val::U32(5)]), Val::Bool(true));
    assert_eq!(
        call("find", &[u32s(&[4, 5]), Val::U32(6)]),
        Val::Bool(false)
    );
    assert_eq!(call("siblings", &[u32s(&[1, 2])]), Val::U32(6));
}

#[test]
fn errors() {
    let cases: &[(&str, Code, &str)] = &[
        (
            "local x;",
            Code::Syntax,
            "a local needs a type or an initial value",
        ),
        ("local k = option::none;", Code::CannotInfer, "cannot infer"),
        ("local xs = [];", Code::CannotInfer, "cannot infer"),
        (
            "local r = result::ok(1);",
            Code::CannotInfer,
            "cannot infer",
        ),
        ("local k = 3000000000;", Code::LiteralOutOfRange, "s32"),
        ("local k = 1; local m: u32 = k;", Code::TypeMismatch, "s32"),
        ("local k = u64(0);", Code::UntypedLiteral, "u64"),
        ("local k = 1 + 2.0;", Code::TypeMismatch, "f64"),
        (
            "local k = nothing();",
            Code::TypeMismatch,
            "does not return a value",
        ),
        (
            "local k = 1; local k = 2;",
            Code::DuplicateLocal,
            "already declared",
        ),
        ("local seen = 1;", Code::ShadowsGlobal, "global"),
        ("local q = q;", Code::UnknownName, "`q`"),
        (
            "local (a, b) = stream::new();",
            Code::CannotInfer,
            "cannot infer",
        ),
        (
            "local (a, a) = pair();",
            Code::DuplicateLocal,
            "already declared",
        ),
        ("for (c in s) { }", Code::ForNotList, "string::chars(s)"),
        ("for (i in 10) { }", Code::CannotInfer, "cannot infer"),
        (
            "for (x in n) { }",
            Code::ForNotList,
            "this value is a `u32`",
        ),
        ("for (x in []) { }", Code::CannotInfer, "cannot infer"),
        ("for x in xs { }", Code::Syntax, "`(`"),
        ("for (x: u32 in xs) { }", Code::Syntax, "`in`"),
        ("for (x in xs) return;", Code::Syntax, "braces are required"),
        (
            "for (n in xs) { }",
            Code::DuplicateLocal,
            "already declared",
        ),
        ("for (seen in xs) { }", Code::ShadowsGlobal, "global"),
        (
            "for (x in xs) { local x: u32 = 1; }",
            Code::DuplicateLocal,
            "already declared",
        ),
        ("for (x in xs) { } x = 1;", Code::UnknownName, "`x`"),
        ("for (x in xs) { x = \"a\"; }", Code::TypeMismatch, "u32"),
        ("local in: u32 = 1;", Code::ReservedKeyword, "`in`"),
        (
            "break;",
            Code::BreakOutsideLoop,
            "inside a `while` or a `for`",
        ),
    ];
    for (body, code, message) in cases {
        let program = format!(
            "world w {{ export t; }}\nglobal seen: mut u32 = 0;\ninterface t {{\n  func nothing() {{ }}\n  func pair() -> tuple<u32, string> {{ return tuple(3, \"abc\"); }}\n  func f(n: u32, s: string, xs: list<u32>) {{\n    {body}\n  }}\n}}\n"
        );
        let mut sources = SourceMap::default();
        sources.add("src/t.wasl".into(), program);
        let error = match waslc::analyze(&package(), &sources, &Default::default()) {
            Ok(_) => panic!("`{body}` is accepted"),
            Err(mut errors) => errors.remove(0),
        };
        let shown = error.display(&sources).to_string();
        assert_eq!(error.code, *code, "`{body}`: {shown}");
        assert!(shown.contains(message), "`{body}`: {shown}");
    }
}
