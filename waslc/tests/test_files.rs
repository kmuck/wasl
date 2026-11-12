//! Test files (specification, § 3.6): analyzed with the package, built into the
//! component that runs the tests, and their errors.

use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Engine, Store};

use waslc::diag::Code;
use waslc::{Package, SourceMap};

const SOURCE: &str = r#"
world w {
  export shop;
}

global calls: mut u32 = 0;

interface shop {
  record line { sku: string, quantity: u32, unit_price: u64 }

  func total(lines: list<line>) -> u64 {
    calls = calls + 1;
    local total: u64 = 0;
    for (l in lines)
      total = total + u64(l.quantity) * l.unit_price;
    return total;
  }

  func calls_so_far() -> u32 { return calls; }
}

// Not exported by a world: compiled into the tests all the same.
interface internal {
  func half(n: u32) -> u32 { return n / 2; }
}
"#;

const TESTS: &str = r#"
use wasl:std/text;

interface shop_test {
  func total_of_two_lines() -> result<_, string> {
    local lines = [
      shop::line { sku: "pen", quantity: 3, unit_price: 150 },
      shop::line { sku: "ink", quantity: 1, unit_price: 900 },
    ];
    local total = shop::total(lines);
    if (total != 1350)
      return result.err("expected 1350, got " + u64.to_string(total));
    return result.ok;
  }

  func wrong_total() -> result<_, string> {
    return check(shop::total([]), 1);
  }

  // Each test runs in a new instance: the global starts from its initial value.
  func fresh_instance() -> result<_, string> {
    shop::total([]);
    return check(u64(shop::calls_so_far()), 1);
  }

  func traps() -> result<_, string> {
    local zero: u32 = 0;
    internal::half(1 / zero);
    return result.ok;
  }

  // Helpers: not tests, and they may use types that cannot be in the WIT.
  func check(got: u64, expected: u64) -> result<_, string> {
    if (got != expected)
      return result.err("expected " + u64.to_string(expected) + ", got " + u64.to_string(got));
    return result.ok;
  }
  func buffer() -> list<mut u8> { return list.filled(2, 0); }
  func number() -> u32 { return 1; }
}
"#;

const MORE_TESTS: &str = r#"
use wasl:std/text;

interface text_test {
  func uses_the_library() -> result<_, string> {
    if (text::trim("  a ") != "a")
      return result.err("trim");
    return result.ok;
  }
  // A test interface calls another one.
  func calls_another_test() -> result<_, string> {
    return shop_test::check(u64(list.len(shop_test::buffer())), 2);
  }
}
"#;

fn package() -> Package {
    Package::new("test", "shop", "0.1.0").unwrap()
}

fn sources(source: &str, tests: &[&str]) -> SourceMap {
    let mut map = SourceMap::default();
    map.add("src/shop.wasl".into(), source.to_string());
    for (k, text) in tests.iter().enumerate() {
        map.add_test(format!("tests/t{k}.wasl").into(), text.to_string());
    }
    map
}

fn analyze(source: &str, tests: &[&str]) -> Result<waslc::Analysis, waslc::Diagnostic> {
    waslc::analyze(&package(), &sources(source, tests), &Default::default())
        .map_err(|mut e| e.remove(0))
}

#[test]
fn runs_each_test_in_a_new_instance() {
    let map = sources(SOURCE, &[TESTS, MORE_TESTS]);
    let analysis = waslc::analyze(&package(), &map, &Default::default())
        .unwrap_or_else(|mut e| panic!("{}", e.remove(0).display(&map)));
    let names: Vec<&str> = analysis.tests().collect();
    // `check` returns `result<_, string>` but has a parameter: a helper.
    assert_eq!(
        names,
        [
            "shop_test::total_of_two_lines",
            "shop_test::wrong_total",
            "shop_test::fresh_instance",
            "shop_test::traps",
            "text_test::uses_the_library",
            "text_test::calls_another_test",
        ]
    );
    // The worlds of the package are those of the sources.
    assert_eq!(analysis.worlds().len(), 1);

    let (bytes, interface) = analysis
        .build_tests()
        .unwrap_or_else(|e| panic!("{}", e.display(&map)));
    assert_eq!(interface, "test:shop/wasl-tests@0.1.0");
    let engine = Engine::default();
    let component = Component::new(&engine, &bytes).unwrap();
    let run = |k: usize| {
        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine)
            .instantiate(&mut store, &component)
            .unwrap();
        let iface = instance
            .get_export_index(&mut store, None, &interface)
            .unwrap();
        let index = instance
            .get_export_index(&mut store, Some(&iface), &format!("t{k}"))
            .unwrap();
        let func = instance.get_func(&mut store, index).unwrap();
        let mut results = [Val::Bool(false)];
        func.call(&mut store, &[], &mut results)
            .map(|()| results[0].clone())
            .map_err(|e| format!("{e:?}"))
    };
    let ok = Ok(Val::Result(Ok(None)));
    let err = |m: &str| Ok(Val::Result(Err(Some(Box::new(Val::String(m.into()))))));
    assert_eq!(run(0), ok);
    assert_eq!(run(1), err("expected 1, got 0"));
    assert_eq!(run(2), ok);
    assert_eq!(run(2), ok);
    assert!(run(3).is_err());
    assert_eq!(run(4), ok);
    assert_eq!(run(5), ok);
}

#[test]
fn without_test_files() {
    let analysis = analyze(SOURCE, &[]).unwrap();
    assert_eq!(analysis.tests().count(), 0);
    assert!(analysis.build_tests().is_err());
    // A test file without test.
    let analysis = analyze(
        SOURCE,
        &["interface empty_test { func f() -> u32 { return 1; } }"],
    )
    .unwrap();
    assert_eq!(analysis.tests().count(), 0);
    assert!(analysis.build_tests().is_ok());
}

#[test]
fn errors() {
    let cases: &[(&str, &str, Code, &str)] = &[
        (
            SOURCE,
            "world helpers { }",
            Code::TestFileItem,
            "`helpers` is a world, but a test file only declares interfaces",
        ),
        (
            SOURCE,
            "global g: u32 = 0;",
            Code::TestFileItem,
            "`g` is a global",
        ),
        (
            SOURCE,
            "interface shop { }",
            Code::DuplicateItem,
            "`shop` is already declared as an interface",
        ),
        (
            SOURCE,
            "interface a { } interface a { }",
            Code::DuplicateItem,
            "already declared as an interface of the tests",
        ),
        // The package does not see the interfaces of the tests.
        (
            "world w { export t; } interface t { func f() -> u32 { return t_test::g(); } }",
            "interface t_test { func g() -> u32 { return 1; } }",
            Code::UnknownName,
            "t_test",
        ),
        (
            "world w { export t_test; } interface t { }",
            "interface t_test { func g() -> u32 { return 1; } }",
            Code::UnknownExportedInterface,
            "t_test",
        ),
        (
            SOURCE,
            "use wasl:std/text as a; interface a { }",
            Code::InterfaceNameClash,
            "like an interface of the tests",
        ),
    ];
    for (source, tests, code, message) in cases {
        let map = sources(source, &[tests]);
        let error = match waslc::analyze(&package(), &map, &Default::default()) {
            Ok(_) => panic!("`{tests}` is accepted"),
            Err(mut errors) => errors.remove(0),
        };
        let shown = error.display(&map).to_string();
        assert_eq!(error.code, *code, "`{tests}`: {shown}");
        assert!(shown.contains(message), "`{tests}`: {shown}");
    }
}
