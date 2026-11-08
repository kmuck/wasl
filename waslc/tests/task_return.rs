//! `task::return` (specification, § 8.21, § 12.12): an async function that delivers its
//! result before its end, run by wasmtime; and the errors E0417 to E0419.

use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use wasmtime::component::{
    Component, Linker, Resource, ResourceType, Source, StreamConsumer, StreamResult, Val,
};
use wasmtime::{Engine, Store, StoreContextMut};

use waslc::{Dependencies, Package, SourceMap, diag::Code};

/// A host resource, lent to the component.
const HOST: &str = r#"
package demo:host@1.0.0;

interface data {
  resource blob {
    size: func() -> u32;
  }
}
"#;

struct Blob;

fn deps() -> Dependencies {
    let mut deps = Dependencies::default();
    deps.add_wit("demo:host", "host.wit", HOST).unwrap();
    deps
}

fn sources(source: &str) -> SourceMap {
    let mut sources = SourceMap::default();
    sources.add("src/main.wasl".into(), source.to_string());
    sources
}

fn analyze(source: &str) -> waslc::Result<waslc::Analysis> {
    let package = Package::new("test", "app", "0.1.0").unwrap();
    waslc::analyze(&package, &sources(source), &deps()).map_err(|mut e| e.remove(0))
}

fn build(source: &str) -> Vec<u8> {
    analyze(source)
        .and_then(|analysis| analysis.build("w"))
        .unwrap_or_else(|e| panic!("{}", e.display(&sources(source))))
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

/// Runs a future to its end, polling it again and again.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

/// A host reader of a stream: collects its bytes, and records its end when wasmtime
/// drops it.
struct Collect {
    bytes: Arc<Mutex<Vec<u8>>>,
    done: Arc<Mutex<bool>>,
}

impl<D> StreamConsumer<D> for Collect {
    type Item = u8;

    fn poll_consume(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        store: StoreContextMut<D>,
        source: Source<'_, u8>,
        _finish: bool,
    ) -> Poll<wasmtime::Result<StreamResult>> {
        let mut source = source.as_direct(store);
        let n = source.remaining().len();
        self.bytes
            .lock()
            .unwrap()
            .extend_from_slice(source.remaining());
        source.mark_read(n);
        Poll::Ready(Ok(StreamResult::Completed))
    }
}

impl Drop for Collect {
    fn drop(&mut self) {
        *self.done.lock().unwrap() = true;
    }
}

/// The component of `source`, instantiated with the host of [`HOST`].
fn instantiate(source: &str) -> (Store<()>, wasmtime::component::Instance) {
    let bytes = build(source);
    let mut config = wasmtime::Config::new();
    config.wasm_gc(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::new(&engine, &bytes).unwrap();
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    let mut data = linker.instance("demo:host/data@1.0.0").unwrap();
    data.resource("blob", ResourceType::host::<Blob>(), |_, _| Ok(()))
        .unwrap();
    data.func_wrap(
        "[method]blob.size",
        |_: StoreContextMut<'_, ()>, (blob,): (Resource<Blob>,)| Ok((blob.rep(),)),
    )
    .unwrap();
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    (store, instance)
}

/// Delivers a stream, then writes to it: the caller has the stream before the bytes.
const COUNT: &str = r#"
world w {
  export numbers;
}

interface numbers {
  async func count(n: u8) -> result<stream<u8>, string> {
    if (n == 0) {
      return result::err("nothing to count");
    }
    local (rx: stream<u8>, tx: stream_writer<u8>) = stream::new();
    _ = numbers::check(n)?;
    task::return(result::ok(rx));

    local i: u8 = 1;
    while (i <= n) {
      _ = stream::write(tx, [i]);
      i = i + 1;
    }
    stream::close(tx);
  }

  func check(n: u8) -> result<u8, string> {
    if (n > 100) {
      return result::err("too many");
    }
    return result::ok(n);
  }
}
"#;

/// Calls `count(n)`: its result, and the bytes of its stream.
fn count(n: u8) -> (Result<(), String>, Vec<u8>) {
    let (mut store, instance) = instantiate(COUNT);
    let iface = instance
        .get_export_index(&mut store, None, "test:app/numbers@0.1.0")
        .unwrap();
    let index = instance
        .get_export_index(&mut store, Some(&iface), "count")
        .unwrap();
    let func = instance.get_func(&mut store, index).unwrap();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(Mutex::new(false));
    let (collected, finished) = (bytes.clone(), done.clone());
    let result = block_on(store.run_concurrent(async move |accessor| {
        let mut results = vec![Val::Bool(false)];
        func.call_concurrent(accessor, &[Val::U8(n)], &mut results)
            .await
            .unwrap();
        // The call returns at `task::return`: the task goes on and writes the stream.
        let stream = match results.remove(0) {
            Val::Result(Ok(Some(stream))) => *stream,
            Val::Result(Err(Some(e))) => return Err(format!("{e:?}")),
            other => panic!("{other:?}"),
        };
        let Val::Stream(stream) = stream else {
            panic!("{stream:?}")
        };
        accessor.with(|mut store| {
            stream
                .try_into_stream_reader::<u8>()
                .unwrap()
                .pipe(
                    &mut store,
                    Collect {
                        bytes: collected,
                        done: finished,
                    },
                )
                .unwrap();
        });
        poll_fn(|cx| {
            if *done.lock().unwrap() {
                Poll::Ready(())
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await;
        Ok(())
    }))
    .unwrap();
    let bytes = bytes.lock().unwrap().clone();
    (result, bytes)
}

#[test]
fn delivered_stream() {
    assert_eq!(count(3), (Ok(()), vec![1, 2, 3]));
    assert_eq!(count(100).1.len(), 100);
}

#[test]
fn errors_before_task_return() {
    // `return e;` and `?` before `task::return` deliver their value.
    let (result, bytes) = count(0);
    assert!(result.unwrap_err().contains("nothing to count"));
    assert!(bytes.is_empty());
    let (result, _) = count(200);
    assert!(result.unwrap_err().contains("too many"));
}

#[test]
fn borrows_given_back() {
    // The Component Model traps at `task.return` while the task holds a borrow: the
    // borrowed handle is given back before the result is delivered.
    let source = r#"
        world w {
          import demo:host/data;
          export sizes;
        }

        interface sizes {
          async func measure(blob: borrow<data::blob>) -> u32 {
            local size: u32 = blob.size();
            task::return(size * 2);
            local unused: u32 = size;
          }
        }"#;
    let (mut store, instance) = instantiate(source);
    let iface = instance
        .get_export_index(&mut store, None, "test:app/sizes@0.1.0")
        .unwrap();
    let index = instance
        .get_export_index(&mut store, Some(&iface), "measure")
        .unwrap();
    let measure = instance
        .get_typed_func::<(Resource<Blob>,), (u32,)>(&mut store, index)
        .unwrap();
    let (size,) = block_on(store.run_concurrent(async move |accessor| {
        measure
            .call_concurrent(accessor, (Resource::new_borrow(21),))
            .await
    }))
    .unwrap()
    .unwrap();
    assert_eq!(size, 42);
}

#[test]
fn wit() {
    // A function that delivers its result has the WIT of any async function.
    let package = Package::new("test", "app", "0.1.0").unwrap();
    let wit = waslc::analyze(&package, &sources(COUNT), &deps())
        .unwrap()
        .wit()
        .unwrap();
    assert!(
        wit.contains("count: async func(n: u8) -> result<stream<u8>, string>;"),
        "{wit}"
    );
}

/// An async function `f` of interface `t` exported by world `w`, with `body`.
fn program(body: &str) -> String {
    format!(
        "world w {{
           import demo:host/data;
           export t;
         }}
         interface t {{
           async func f(blob: borrow<data::blob>, n: u32) -> result<u32, string> {{
             {body}
           }}
           func g(n: u32) -> result<u32, string> {{
             return result::ok(n);
           }}
         }}"
    )
}

#[test]
fn accepted() {
    // `return;` after `task::return`, and reaching the end of the body.
    for body in [
        "task::return(result::ok(n));",
        "task::return(result::ok(n)); return;",
        "if (n == 0) { return result::err(\"zero\"); } \
         local m: u32 = t::g(n)?; \
         task::return(result::ok(m)); \
         if (m > 1) { return; } \
         while (false) { return; }",
    ] {
        analyze(&program(body)).unwrap_or_else(|e| panic!("{body}: {}", e.message));
    }
}

#[test]
fn errors() {
    let delivered = "task::return(result::ok(n));";
    // E0417: inside a block, or twice; the value has the return type.
    fails(
        &program(&format!(
            "if (n == 0) {{ {delivered} }} return result::ok(n);"
        )),
        Code::TaskReturnPlace,
        "only allowed at the top level of the body",
    );
    fails(
        &program(&format!("while (true) {{ {delivered} }}")),
        Code::TaskReturnPlace,
        "only allowed at the top level of the body",
    );
    fails(
        &program(&format!("{delivered} {delivered}")),
        Code::TaskReturnPlace,
        "already delivered",
    );
    fails(
        &program("task::return(1);"),
        Code::TypeMismatch,
        "integer literal used for type `result<u32, string>`",
    );
    // E0418: `return e;`, `?` and a `borrow` parameter after `task::return`.
    fails(
        &program(&format!("{delivered} return result::err(\"late\");")),
        Code::AfterTaskReturn,
        "only `return;` can end the function",
    );
    fails(
        &program(&format!(
            "{delivered} if (n > 1) {{ local m: u32 = t::g(n)?; }}"
        )),
        Code::AfterTaskReturn,
        "`?` after `task::return`",
    );
    fails(
        &program(&format!("{delivered} local size: u32 = blob.size();")),
        Code::AfterTaskReturn,
        "`blob` is a `borrow` parameter, used after `task::return`",
    );
    // `task::return` is a statement.
    fails(
        &program("local x: u32 = task::return(result::ok(n));"),
        Code::Syntax,
        "statement of its own",
    );
    fails(
        &program("task::finish(1);"),
        Code::Syntax,
        "`task::return(e);`",
    );
    // `task` is a keyword.
    fails(
        &program("local task: u32 = 1; return result::ok(task);"),
        Code::ReservedKeyword,
        "`task` is a keyword",
    );
}

#[test]
fn place_errors() {
    // E0417: a function without a return type, or that is not async.
    fails(
        "world w { export t; }
         interface t {
           async func f() { task::return(1); }
         }",
        Code::TaskReturnPlace,
        "has no return type",
    );
    fails(
        "world w { export t; }
         interface t {
           func f() -> u32 { task::return(1); }
         }",
        Code::TaskReturnPlace,
        "only allowed in an async function",
    );
}

#[test]
fn entry_calls() {
    // E0419: a function that contains `task::return` is called by the host and other
    // components only.
    let source = |caller: &str| {
        format!(
            "world w {{ export t; }}
             interface t {{
               async func f(n: u32) -> u32 {{
                 task::return(n);
               }}
               {caller}
             }}"
        )
    };
    fails(
        &source("async func g() -> u32 { return t::f(1); }"),
        Code::EntryCall,
        "`t::g` calls `t::f`, which contains `task::return`",
    );
    // A function that no world reaches is not compiled, and is no error.
    let unreached = "world w { export u; }
         interface u { func h() -> u32 { return 1; } }
         interface t {
           async func f(n: u32) -> u32 { task::return(n); }
           async func g() -> u32 { return t::f(1); }
         }";
    analyze(unreached).unwrap_or_else(|e| panic!("{}", e.message));
    // A call to it in another component goes through an import.
    let other = "world w { export t; }
         world client { import t; export c; }
         interface t {
           async func f(n: u32) -> u32 { task::return(n); }
         }
         interface c {
           async func g() -> u32 { return t::f(1); }
         }";
    analyze(other).unwrap_or_else(|e| panic!("{}", e.message));
}
