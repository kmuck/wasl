//! Implementations of interfaces of other packages (specification, § 2.4.2, § 13.12):
//! `examples/hello` against the interfaces of WASI 0.2 that it uses, reduced to what it
//! needs and provided by the host.

use std::sync::{Arc, Mutex};

use wasmtime::component::{Component, Linker, Resource, ResourceType, Val};
use wasmtime::{Engine, Store};

use waslc::{Dependencies, Package, SourceMap, diag::Code};

const HELLO: &str = include_str!("../../examples/hello/src/hello.wasl");

const WASI_IO: &str = r#"
package wasi:io@0.2.6;

interface error {
  resource error {
    to-debug-string: func() -> string;
  }
}

interface streams {
  use error.{error};

  variant stream-error {
    last-operation-failed(error),
    closed,
  }

  resource output-stream {
    blocking-write-and-flush: func(contents: list<u8>) -> result<_, stream-error>;
  }
}
"#;

const WASI_CLI: &str = r#"
package wasi:cli@0.2.6;

interface stdout {
  use wasi:io/streams@0.2.6.{output-stream};
  get-stdout: func() -> output-stream;
}

interface run {
  run: func() -> result;
}
"#;

fn deps() -> Dependencies {
    let mut deps = Dependencies::default();
    deps.add_wit("wasi:io", "io.wit", WASI_IO).unwrap();
    deps.add_wit("wasi:cli", "cli.wit", WASI_CLI).unwrap();
    deps
}

fn analyze(source: &str) -> waslc::Result<waslc::Analysis> {
    let package = Package::new("example", "hello", "0.1.0").unwrap();
    let mut sources = SourceMap::default();
    sources.add("src/hello.wasl".into(), source.to_string());
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

struct Stream;

/// The host side: what the component writes, and the streams it gives back.
#[derive(Default)]
struct Host {
    out: Arc<Mutex<Vec<u8>>>,
    dropped: u32,
}

#[test]
fn hello() {
    let mut sources = SourceMap::default();
    sources.add("src/hello.wasl".into(), HELLO.to_string());
    let analysis = analyze(HELLO).unwrap_or_else(|e| panic!("{}", e.display(&sources)));
    let wit = analysis.wit().unwrap();
    assert!(wit.contains("export wasi:cli/run@0.2.6;"), "{wit}");
    assert!(!wit.contains("interface run"), "{wit}");
    let bytes = analysis.build("hello").unwrap();

    let engine = Engine::default();
    let component = Component::new(&engine, &bytes).unwrap();
    let mut linker: Linker<Host> = Linker::new(&engine);
    let mut streams = linker.instance("wasi:io/streams@0.2.6").unwrap();
    streams
        .resource(
            "output-stream",
            ResourceType::host::<Stream>(),
            |mut s, _| {
                s.data_mut().dropped += 1;
                Ok(())
            },
        )
        .unwrap();
    streams
        .func_new(
            "[method]output-stream.blocking-write-and-flush",
            |mut store, _, args, out| {
                // A dynamic host function ends the borrow of `self` by converting it.
                let Val::Resource(this) = &args[0] else {
                    panic!()
                };
                let _: Resource<Stream> = Resource::try_from_resource_any(*this, &mut store)?;
                let Val::List(bytes) = &args[1] else { panic!() };
                let mut sink = store.data().out.lock().unwrap();
                for b in bytes {
                    let Val::U8(b) = b else { panic!() };
                    sink.push(*b);
                }
                out[0] = Val::Result(Ok(None));
                Ok(())
            },
        )
        .unwrap();
    let mut errors = linker.instance("wasi:io/error@0.2.6").unwrap();
    errors
        .resource("error", ResourceType::host::<u32>(), |_, _| Ok(()))
        .unwrap();
    let mut stdout = linker.instance("wasi:cli/stdout@0.2.6").unwrap();
    stdout
        .func_new("get-stdout", |mut store, _, _, out| {
            let r = Resource::<Stream>::new_own(1);
            out[0] = Val::Resource(r.try_into_resource_any(&mut store).unwrap());
            Ok(())
        })
        .unwrap();

    let mut store = Store::new(&engine, Host::default());
    let instance = linker.instantiate(&mut store, &component).unwrap();
    let iface = instance
        .get_export_index(&mut store, None, "wasi:cli/run@0.2.6")
        .expect("wasi:cli/run is exported");
    let index = instance
        .get_export_index(&mut store, Some(&iface), "run")
        .unwrap();
    let run = instance.get_func(&mut store, index).unwrap();
    let mut result = [Val::Bool(false)];
    run.call(&mut store, &[], &mut result).unwrap();
    assert_eq!(result[0], Val::Result(Ok(None)));
    let out = store.data().out.lock().unwrap().clone();
    assert_eq!(String::from_utf8(out).unwrap(), "Hello, world!\n");
    // The stream was given back when `run` returned.
    assert_eq!(store.data().dropped, 1);
}

#[test]
fn errors() {
    fails(
        "world w { export wasi:cli/run; }",
        Code::Syntax,
        "needs its implementation in the world",
    );
    fails(
        "world w { export wasi:cli/run { } }",
        Code::ForeignExport,
        "the function `run` of `wasi:cli/run` is missing: `func run() -> result`",
    );
    fails(
        "world w { export wasi:cli/run { func run() -> u32 { return 0; } } }",
        Code::ForeignExport,
        "`run` does not have the signature of `wasi:cli/run`: expected `func run() -> result`, found `func run() -> u32`",
    );
    fails(
        "world w { export wasi:cli/run {
           func run() -> result { return result::ok; }
           func other() { }
         } }",
        Code::ForeignExport,
        "`other` is not a function of `wasi:cli/run`",
    );
    fails(
        "world w { export wasi:io/streams { } }",
        Code::ForeignExport,
        "declares the resource `output_stream`",
    );
    fails(
        "world w {
           export wasi:cli/run { func run() -> result { return result::ok; } }
           export wasi:cli/run { func run() -> result { return result::ok; } }
         }",
        Code::DuplicateExport,
        "already exported",
    );
    fails(
        "world w {
           import wasi:cli/run;
           export wasi:cli/run { func run() -> result { return result::ok; } }
         }",
        Code::DuplicateImport,
        "so it cannot export it",
    );
    // A block on an import or a local interface is a syntax error.
    fails("world w { import wasi:cli/run { } }", Code::Syntax, "`;`");
    // Each world has its own implementation, which can call the interfaces of the
    // package.
    assert!(
        analyze(
            "world w { export wasi:cli/run {
               func run() -> result { if (m::two() == 2) { return result::ok; } return result::err; }
             } }
             world v { export wasi:cli/run { func run() -> result { return result::err; } } }
             interface m { func two() -> u32 { return 2; } }"
        )
        .is_ok()
    );
}
