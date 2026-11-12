//! The standard library (specification, chapter 17): `wasl:std/text`, compiled into the
//! component that uses it, run by wasmtime; and the rules of `use wasl:std/…` (§ 3.5).

use std::sync::OnceLock;

use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Engine, Store};

use waslc::{Dependencies, Package, SourceMap};

const INTERFACE: &str = "test:std/t@0.1.0";

const PROGRAM: &str = r#"
use wasl:std/text;

world w {
  export t;
}

// The library does not see the globals of the package: its locals keep their names.
global out: u32 = 0;

// An interface of the package may have the name of an interface of the library.
interface t {
  func split(s: string, sep: string) -> list<string> { return text::split(s, sep); }
  func join(parts: list<string>, sep: string) -> string { return text::join(parts, sep); }
  func lines(s: string) -> list<string> { return text::lines(s); }
  func trim(s: string) -> string { return text::trim(s); }
  func starts_with(s: string, p: string) -> bool { return text::starts_with(s, p); }
  func ends_with(s: string, p: string) -> bool { return text::ends_with(s, p); }
  func contains(s: string, p: string) -> bool { return text::contains(s, p); }
  func find(s: string, p: string) -> option<u32> { return text::find(s, p); }
  func replace(s: string, a: string, b: string) -> string { return text::replace(s, a, b); }
  func slice(s: string, a: u32, b: u32) -> option<string> { return text::slice(s, a, b); }
  func lower(s: string) -> string { return text::lower(s); }
  func upper(s: string) -> string { return text::upper(s); }
}

interface internal {
  func f() -> u32 { return 1; }
}
"#;

fn package() -> Package {
    Package::new("test", "std", "0.1.0").unwrap()
}

fn analyze(files: &[(&str, &str)], deps: &Dependencies) -> Result<waslc::Analysis, String> {
    let mut sources = SourceMap::default();
    for (path, text) in files {
        sources.add((*path).into(), text.to_string());
    }
    waslc::analyze(&package(), &sources, deps).map_err(|mut e| {
        let e = e.remove(0);
        format!("{:?}: {}", e.code, e.display(&sources))
    })
}

fn engine_and_component() -> &'static (Engine, Component) {
    static CELL: OnceLock<(Engine, Component)> = OnceLock::new();
    CELL.get_or_init(|| {
        let analysis = analyze(&[("src/t.wasl", PROGRAM)], &Default::default())
            .unwrap_or_else(|e| panic!("{e}"));
        let bytes = analysis.build("w").unwrap_or_else(|e| panic!("{e:?}"));
        let engine = Engine::default();
        let component = Component::new(&engine, &bytes).unwrap();
        (engine, component)
    })
}

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

fn s(text: &str) -> Val {
    Val::String(text.into())
}

fn strings(items: &[&str]) -> Val {
    Val::List(items.iter().map(|t| s(t)).collect())
}

fn some(v: Val) -> Val {
    Val::Option(Some(Box::new(v)))
}

const NONE: Val = Val::Option(None);

#[test]
fn text() {
    assert_eq!(
        call("split", &[s("a,b,,c"), s(",")]),
        strings(&["a", "b", "", "c"])
    );
    assert_eq!(call("split", &[s(""), s(",")]), strings(&[""]));
    assert_eq!(
        call("split", &[s("a=>b=>"), s("=>")]),
        strings(&["a", "b", ""])
    );
    assert_eq!(call("split", &[s("abc"), s("")]), strings(&["abc"]));
    assert_eq!(call("join", &[strings(&["a", "b"]), s(", ")]), s("a, b"));
    assert_eq!(call("join", &[strings(&[]), s(", ")]), s(""));
    assert_eq!(call("lines", &[s("a\r\nb\n")]), strings(&["a", "b"]));
    assert_eq!(call("lines", &[s("a\n\nb")]), strings(&["a", "", "b"]));
    assert_eq!(call("lines", &[s("")]), strings(&[]));
    assert_eq!(call("trim", &[s(" \t héllo \r\n")]), s("héllo"));
    assert_eq!(call("trim", &[s("   ")]), s(""));
    assert_eq!(call("starts-with", &[s("hello"), s("he")]), Val::Bool(true));
    assert_eq!(
        call("starts-with", &[s("he"), s("hello")]),
        Val::Bool(false)
    );
    assert_eq!(call("ends-with", &[s("hello"), s("llo")]), Val::Bool(true));
    assert_eq!(call("ends-with", &[s("lo"), s("hello")]), Val::Bool(false));
    assert_eq!(call("contains", &[s("hello"), s("ell")]), Val::Bool(true));
    assert_eq!(call("contains", &[s("hello"), s("elk")]), Val::Bool(false));
    assert_eq!(call("find", &[s("héllo"), s("llo")]), some(Val::U32(3)));
    assert_eq!(call("find", &[s("hello"), s("x")]), NONE);
    assert_eq!(
        call("replace", &[s("a-b-c"), s("-"), s("--")]),
        s("a--b--c")
    );
    assert_eq!(call("replace", &[s("aaa"), s("aa"), s("b")]), s("ba"));
    assert_eq!(call("replace", &[s("abc"), s(""), s("x")]), s("abc"));
    assert_eq!(
        call("slice", &[s("héllo"), Val::U32(3), Val::U32(6)]),
        some(s("llo"))
    );
    assert_eq!(call("slice", &[s("héllo"), Val::U32(2), Val::U32(4)]), NONE);
    assert_eq!(call("slice", &[s("abc"), Val::U32(2), Val::U32(1)]), NONE);
    assert_eq!(call("slice", &[s("abc"), Val::U32(1), Val::U32(4)]), NONE);
    assert_eq!(call("lower", &[s("HeLLo, É!")]), s("hello, É!"));
    assert_eq!(call("upper", &[s("HeLLo, é!")]), s("HELLO, é!"));
}

#[test]
fn std_rules() {
    let fails = |files: &[(&str, &str)], deps: &Dependencies, code: &str, message: &str| {
        let e = match analyze(files, deps) {
            Ok(_) => panic!("accepted: {files:?}"),
            Err(e) => e,
        };
        assert!(e.starts_with(code), "{e}");
        assert!(e.contains(message), "{e}");
    };
    let none = Dependencies::default();
    fails(
        &[(
            "src/a.wasl",
            "use wasl:std/nope;\nworld w { export t; }\ninterface t { }",
        )],
        &none,
        "ForeignInterface",
        "the standard library `wasl:std` has no interface `nope`; its interfaces are",
    );
    fails(
        &[(
            "src/a.wasl",
            "world w { export t; }\ninterface t { func f() -> list<string> { return text::split(\"a\", \",\"); } }",
        )],
        &none,
        "UnknownName",
        "unknown interface or type `text`",
    );
    fails(
        &[("src/a.wasl", "use wasl:std/text;\nworld w { export text; }")],
        &none,
        "ForeignExport",
        "standard library `wasl:std/text`",
    );
    let mut with_std = Dependencies::default();
    with_std
        .add_wit(
            "wasl:std",
            "std.wit",
            "package wasl:std@0.5.0;\ninterface x { }",
        )
        .unwrap();
    fails(&[("src/a.wasl", "")], &with_std, "Manifest", "remove it");
}
