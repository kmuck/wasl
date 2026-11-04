//! Imports (specification, § 2.5 and § 13.10): calls to the functions of interfaces of
//! other packages, provided here by the host, and imports of interfaces of the package.

use wasmtime::component::{Component, Instance, Linker, Val};
use wasmtime::{Engine, Store};

use waslc::{Dependencies, Package, SourceMap, diag::Code};

/// The WIT of the dependency `demo:host`, provided by the host in these tests.
const HOST: &str = r#"
package demo:host@1.0.0;

interface log {
  enum level { info, warn }
  record entry { level: level, text: string, tags: list<string> }
  variant event { started(u32), message(string), point(tuple<f32, f64>), nothing }
  flags opts { quiet, verbose }
  type instant = u64;

  write: func(e: entry) -> result<u32, string>;
  add: func(a: u32, b: u32) -> u32;
  echo: func(s: string) -> string;
  pair: func(x: u64) -> tuple<u64, string>;
  maybe: func(o: option<string>) -> option<u32>;
  describe: func(e: event) -> string;
  now: func() -> instant;
  options: func(o: opts) -> opts;
  sum: func(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8, i: u8, j: u8, k: u8, l: u8, m: u8, n: u8, o: u8, p: u8, q: u8) -> u32;
  reset: func();
}

interface files {
  resource file;
  open: func(path: string) -> file;
  wait: async func();
  count: func() -> u32;
  %from: func() -> u32;
  get-UPPER: func() -> u32;
}

interface types-only {
  record point { x: s32, y: s32 }
}
"#;

fn deps() -> Dependencies {
    let mut deps = Dependencies::default();
    deps.add_wit("demo:host", "host.wit", HOST).unwrap();
    deps
}

fn analyze(source: &str) -> waslc::Result<waslc::Analysis> {
    let package = Package::new("test", "app", "0.1.0").unwrap();
    let mut sources = SourceMap::default();
    sources.add("src/main.wasl".into(), source.to_string());
    waslc::analyze(&package, &sources, &deps())
}

fn build(source: &str, world: &str) -> Vec<u8> {
    let package = Package::new("test", "app", "0.1.0").unwrap();
    let mut sources = SourceMap::default();
    sources.add("src/main.wasl".into(), source.to_string());
    waslc::analyze(&package, &sources, &deps())
        .and_then(|analysis| analysis.build(world))
        .unwrap_or_else(|e| panic!("{}", e.display(&sources)))
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

fn string(s: &str) -> Val {
    Val::String(s.into())
}

/// The host side of `demo:host/log`.
fn host(linker: &mut Linker<()>) {
    let mut log = linker.instance("demo:host/log@1.0.0").unwrap();
    log.func_new("write", |_, _, params, results| {
        let Val::Record(fields) = &params[0] else {
            panic!()
        };
        let text = match &fields[1].1 {
            Val::String(s) => s.clone(),
            _ => panic!(),
        };
        let Val::List(tags) = &fields[2].1 else {
            panic!()
        };
        results[0] = match &fields[0].1 {
            Val::Enum(level) if level == "warn" => Val::Result(Err(Some(Box::new(string(
                &format!("warning: {text} ({} tags)", tags.len()),
            ))))),
            _ => Val::Result(Ok(Some(Box::new(Val::U32(text.len() as u32))))),
        };
        Ok(())
    })
    .unwrap();
    log.func_new("add", |_, _, params, results| {
        let (Val::U32(a), Val::U32(b)) = (&params[0], &params[1]) else {
            panic!()
        };
        results[0] = Val::U32(a + b);
        Ok(())
    })
    .unwrap();
    log.func_new("echo", |_, _, params, results| {
        let Val::String(s) = &params[0] else { panic!() };
        results[0] = string(&format!("<{s}>"));
        Ok(())
    })
    .unwrap();
    log.func_new("pair", |_, _, params, results| {
        let Val::U64(x) = params[0] else { panic!() };
        results[0] = Val::Tuple(vec![Val::U64(x * 2), string(&x.to_string())]);
        Ok(())
    })
    .unwrap();
    log.func_new("maybe", |_, _, params, results| {
        results[0] = match &params[0] {
            Val::Option(Some(s)) => match &**s {
                Val::String(s) => Val::Option(Some(Box::new(Val::U32(s.len() as u32)))),
                _ => panic!(),
            },
            _ => Val::Option(None),
        };
        Ok(())
    })
    .unwrap();
    log.func_new("describe", |_, _, params, results| {
        let Val::Variant(case, payload) = &params[0] else {
            panic!()
        };
        results[0] = string(&format!("{case} {payload:?}"));
        Ok(())
    })
    .unwrap();
    log.func_new("now", |_, _, _, results| {
        results[0] = Val::U64(1_700_000_000);
        Ok(())
    })
    .unwrap();
    log.func_new("options", |_, _, params, results| {
        let Val::Flags(flags) = &params[0] else {
            panic!()
        };
        results[0] = Val::Flags(if flags.is_empty() {
            vec!["verbose".into()]
        } else {
            Vec::new()
        });
        Ok(())
    })
    .unwrap();
    log.func_new("sum", |_, _, params, results| {
        let total: u32 = params
            .iter()
            .map(|p| match p {
                Val::U8(x) => *x as u32,
                _ => panic!(),
            })
            .sum();
        results[0] = Val::U32(total);
        Ok(())
    })
    .unwrap();
    log.func_new("reset", |_, _, _, _| Ok(())).unwrap();
}

struct Runner {
    store: Store<()>,
    instance: Instance,
}

impl Runner {
    fn new(source: &str, world: &str) -> Runner {
        let bytes = build(source, world);
        let mut config = wasmtime::Config::new();
        config.wasm_gc(true);
        let engine = Engine::new(&config).unwrap();
        let component = Component::new(&engine, &bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let mut linker = Linker::new(&engine);
        host(&mut linker);
        let instance = linker.instantiate(&mut store, &component).unwrap();
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
}

const PROGRAM: &str = r#"
world w {
  import demo:host/log;
  export t;
}

interface t {
  func write(text: string, warn: bool) -> result<u32, string> {
    local level: log::level = log::level::info;
    if (warn) {
      level = log::level::warn;
    }
    return log::write(log::entry { level: level, text: text, tags: ["a", text] });
  }

  // `?` on the result of an import.
  func write_twice(text: string) -> result<u32, string> {
    local n: u32 = log::write(log::entry { level: log::level::info, text: text, tags: [] })?;
    local m: u32 = log::write(log::entry { level: log::level::info, text: text + text, tags: [] })?;
    return result::ok(n + m);
  }

  func add(a: u32, b: u32) -> u32 { return log::add(a, b); }
  func echo(s: string) -> string { return log::echo(log::echo(s)) + "!"; }
  func pair(x: u64) -> string {
    local p: tuple<u64, string> = log::pair(x);
    return p.1 + "/" + log::echo(p.1);
  }
  func maybe(s: string) -> u32 {
    local a: option<u32> = log::maybe(option::some(s));
    local b: option<u32> = log::maybe(option::none);
    match (a) {
      case some(n) {
        match (b) {
          case some(m) { return 0; }
          case none { return n; }
        }
      }
      case none { return 0; }
    }
  }
  func describe(k: u32) -> string {
    if (k == 0) { return log::describe(log::event::started(7)); }
    if (k == 1) { return log::describe(log::event::message("hi")); }
    if (k == 2) { return log::describe(log::event::point(tuple(1.5, 2.25))); }
    return log::describe(log::event::nothing);
  }
  func now() -> u64 {
    local t: log::instant = log::now();
    return t;
  }
  func options() -> bool {
    local o: log::opts = log::options(log::opts { });
    return o.verbose && !o.quiet;
  }
  func sum() -> u32 { return log::sum(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17); }
  func reset() { log::reset(); }
}
"#;

#[test]
fn calls_to_imported_functions() {
    let mut r = Runner::new(PROGRAM, "w");
    assert_eq!(
        r.call("write", &[string("hello"), Val::Bool(false)]),
        Some(Val::Result(Ok(Some(Box::new(Val::U32(5))))))
    );
    assert_eq!(
        r.call("write", &[string("hello"), Val::Bool(true)]),
        Some(Val::Result(Err(Some(Box::new(string(
            "warning: hello (2 tags)"
        ))))))
    );
    assert_eq!(
        r.call("write-twice", &[string("abc")]),
        Some(Val::Result(Ok(Some(Box::new(Val::U32(9))))))
    );
    assert_eq!(
        r.call("add", &[Val::U32(2), Val::U32(40)]),
        Some(Val::U32(42))
    );
    assert_eq!(r.call("echo", &[string("x")]), Some(string("<<x>>!")));
    assert_eq!(r.call("pair", &[Val::U64(21)]), Some(string("21/<21>")));
    assert_eq!(r.call("maybe", &[string("four")]), Some(Val::U32(4)));
    assert_eq!(
        r.call("describe", &[Val::U32(0)]),
        Some(string("started Some(U32(7))"))
    );
    assert_eq!(
        r.call("describe", &[Val::U32(1)]),
        Some(string("message Some(String(\"hi\"))"))
    );
    assert_eq!(
        r.call("describe", &[Val::U32(2)]),
        Some(string("point Some(Tuple([Float32(1.5), Float64(2.25)]))"))
    );
    assert_eq!(
        r.call("describe", &[Val::U32(3)]),
        Some(string("nothing None"))
    );
    assert_eq!(r.call("now", &[]), Some(Val::U64(1_700_000_000)));
    assert_eq!(r.call("options", &[]), Some(Val::Bool(true)));
    assert_eq!(r.call("sum", &[]), Some(Val::U32(153)));
    assert_eq!(r.call("reset", &[]), None);
    // The scratch buffer is reused by every call.
    for _ in 0..100 {
        assert_eq!(r.call("echo", &[string("x")]), Some(string("<<x>>!")));
    }
}

#[test]
fn wit_of_imports() {
    let analysis = analyze(
        "world w { import demo:host/log; export t; }
         interface t { func f(e: log::entry) -> log::level { return e.level; } }",
    )
    .unwrap();
    let wit = analysis.wit().unwrap();
    assert!(
        wit.contains("use demo:host/log@1.0.0.{level, entry};"),
        "{wit}"
    );
    assert!(wit.contains("import demo:host/log@1.0.0;"), "{wit}");
    assert_eq!(analysis.worlds()[0].imports, ["demo:host/log@1.0.0"]);
    build(
        "world w { import demo:host/log; export t; }
         interface t { func f(e: log::entry) -> log::level { return e.level; } }",
        "w",
    );
}

#[test]
fn same_package_imports() {
    let source = "
        world accounts_service { export accounts; }
        world orders_service { import accounts; export orders; }
        global calls: mut u32 = 0;
        interface accounts {
          record account { name: string, balance: u64 }
          func get(name: string) -> account {
            calls = calls + 1;
            return account { name: name, balance: 100 + u64(string::len(name)) - 3 };
          }
          func count() -> u32 { return calls; }
        }
        interface orders {
          func total(name: string) -> u64 { return accounts::get(name).balance + accounts::get(name).balance + 2; }
        }";
    let analysis = analyze(source).unwrap();
    let wit = analysis.wit().unwrap();
    assert!(
        wit.contains("world orders-service {\n  import accounts;\n\n  export orders;"),
        "{wit}"
    );
    // `orders_service` imports `get` instead of compiling it.
    let orders = build(source, "orders_service");
    let text = wasmprinter_like(&orders);
    assert!(text.contains("test:app/accounts@0.1"), "{text}");
    let accounts = build(source, "accounts_service");

    // The two components, linked: the calls of `orders` reach the instance of
    // `accounts`, which keeps its own globals.
    let mut config = wasmtime::Config::new();
    config.wasm_gc(true);
    let engine = Engine::new(&config).unwrap();
    let accounts = Component::new(&engine, &accounts).unwrap();
    let orders = Component::new(&engine, &orders).unwrap();
    let mut store: Store<Option<wasmtime::component::Func>> = Store::new(&engine, None);
    let provider = Linker::new(&engine)
        .instantiate(&mut store, &accounts)
        .unwrap();
    let get = export(&mut store, &provider, "test:app/accounts@0.1.0", "get");
    *store.data_mut() = Some(get);
    let mut linker: Linker<Option<wasmtime::component::Func>> = Linker::new(&engine);
    linker
        .instance("test:app/accounts@0.1.0")
        .unwrap()
        .func_new("get", |mut store, _, params, results| {
            let get = store.data().unwrap();
            get.call(&mut store, params, results)
        })
        .unwrap();
    let consumer = linker.instantiate(&mut store, &orders).unwrap();
    let total = export(&mut store, &consumer, "test:app/orders@0.1.0", "total");
    let calls = export(&mut store, &provider, "test:app/accounts@0.1.0", "count");
    let mut out = [Val::Bool(false)];
    total.call(&mut store, &[string("ada")], &mut out).unwrap();
    assert_eq!(out[0], Val::U64(202));
    calls.call(&mut store, &[], &mut out).unwrap();
    assert_eq!(out[0], Val::U32(2));
}

fn export<T>(
    store: &mut Store<T>,
    instance: &Instance,
    iface: &str,
    name: &str,
) -> wasmtime::component::Func {
    let iface = instance.get_export_index(&mut *store, None, iface).unwrap();
    let index = instance
        .get_export_index(&mut *store, Some(&iface), name)
        .unwrap();
    instance.get_func(&mut *store, index).unwrap()
}

/// The names of the imports of a component, for the assertions.
fn wasmprinter_like(bytes: &[u8]) -> String {
    let mut names = String::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let Ok(wasmparser::Payload::ComponentImportSection(imports)) = payload {
            for import in imports {
                names += import.unwrap().name.name;
                names += "\n";
            }
        }
    }
    names
}

#[test]
fn errors() {
    fails(
        "world w { import wasi:cli/stdout; }",
        Code::ForeignInterface,
        "`wasi:cli` is not a dependency of the package",
    );
    fails(
        "world w { import demo:host/nope; }",
        Code::ForeignInterface,
        "package `demo:host` has no interface `nope`; its interfaces are `files`, `log`, `types_only`",
    );
    fails(
        "world w { import demo:host/log; import demo:host/log; }",
        Code::DuplicateImport,
        "already imported",
    );
    fails(
        "world w { export a; import a; } interface a { }",
        Code::DuplicateImport,
        "exports interface `a`, so it cannot import it",
    );
    fails(
        "world w { import a; export a; } interface a { }",
        Code::DuplicateImport,
        "imports interface `a`, so it cannot export it",
    );
    fails(
        "world w { export demo:host/log; }",
        Code::Syntax,
        "exporting `demo:host/log`, an interface of another package, needs its implementation in the world",
    );
    fails(
        "world w { import nope; }",
        Code::UnknownExportedInterface,
        "unknown interface `nope`",
    );
    fails(
        "world w { import demo:host/log; } interface log { }",
        Code::InterfaceNameClash,
        "`demo:host/log` is named `log` in code, like the interface `log` of the package",
    );
    fails(
        "world w { import demo:host/log; export log; }",
        Code::UnknownExportedInterface,
        "write `demo:host/log`",
    );
    // A call reached from a world that does not import the interface.
    fails(
        "world a { import demo:host/log; export t; }
         world b { export t; }
         interface t { func f() -> u32 { return helper(); } func helper() -> u32 { return log::add(1, 2); } }",
        Code::UnimportedCall,
        "`t::helper` calls `log::add`, of the interface `demo:host/log`, but world `b` reaches `t::helper` and does not import `demo:host/log`",
    );
    // Unreachable code may call it.
    assert!(
        analyze(
            "world b { export t; }
             interface t { func f() -> u32 { return 1; } }
             interface u { func g() -> u32 { return log::add(1, 2); } }
             world a { import demo:host/log; }"
        )
        .is_ok()
    );
    // Items WaSL cannot use.
    fails(
        "world w { import demo:host/files; export t; }
         interface t { func f() { files::wait(); } }",
        Code::ForeignInterface,
        "`files::wait` cannot be used from WaSL: `wait` is an `async` function",
    );
    assert!(
        analyze(
            "world w { import demo:host/files; export t; }
             interface t { func f() -> u32 { return files::count() + files::from(); } }"
        )
        .is_ok()
    );
    // A type of an interface with functions that the world does not import (E0209).
    fails(
        "world w { import demo:host/log; export t; }
         world v { export t; }
         interface t { func f(l: log::level) { } }",
        Code::UnexportedTypeInterface,
        "world `v` does not import interface `demo:host/log`",
    );
    // A types-only interface needs no import.
    assert!(
        analyze(
            "world w { import demo:host/types_only; export t; }
             world v { export t; }
             interface t { func f(p: types_only::point) -> s32 { return p.x; } }"
        )
        .is_ok()
    );
}
