//! Streams and futures (specification, § 7.13, § 8.21, § 12.12): their types, their
//! built-in functions and their errors; programs for WASI 0.3, run by wasmtime with the
//! WASI implementation of `wasmtime-wasi`.

use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use wasmtime::component::{
    Component, FutureConsumer, FutureReader, Linker, ResourceTable, Source, StreamReader, Val,
};
use wasmtime::{Engine, Store, StoreContextMut};
use wasmtime_wasi::p2::pipe::{MemoryInputPipe, MemoryOutputPipe};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use waslc::{Dependencies, Package, SourceMap, diag::Code};

/// The interfaces of `wasi:cli@0.3.0` that these tests use, as WASI publishes them.
const WASI_CLI: &str = r#"
package wasi:cli@0.3.0;

interface run {
  run: async func() -> result;
}

interface types {
  enum error-code {
    io,
    illegal-byte-sequence,
    pipe,
  }
}

interface stdin {
  use types.{error-code};

  read-via-stream: func() -> tuple<stream<u8>, future<result<_, error-code>>>;
}

interface stdout {
  use types.{error-code};

  write-via-stream: func(data: stream<u8>) -> future<result<_, error-code>>;
}
"#;

fn deps() -> Dependencies {
    let mut deps = Dependencies::default();
    deps.add_wit("wasi:cli", "cli.wit", WASI_CLI).unwrap();
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

fn build(source: &str, world: &str) -> Vec<u8> {
    analyze(source)
        .and_then(|analysis| analysis.build(world))
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

struct Wasi {
    ctx: WasiCtx,
    table: ResourceTable,
}

impl WasiView for Wasi {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

/// Runs the `wasi:cli/run` of a command with `stdin` as its standard input: its result,
/// and what it wrote on its standard output.
fn run_command(bytes: &[u8], stdin: &str) -> (Val, String) {
    let mut config = wasmtime::Config::new();
    config.wasm_gc(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::new(&engine, bytes).unwrap();
    let stdout = MemoryOutputPipe::new(1 << 20);
    let ctx = WasiCtx::builder()
        .stdin(MemoryInputPipe::new(stdin.to_string()))
        .stdout(stdout.clone())
        .build();
    let mut store = Store::new(
        &engine,
        Wasi {
            ctx,
            table: ResourceTable::new(),
        },
    );
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::p3::add_to_linker(&mut linker).unwrap();
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let iface = instance
        .get_export_index(&mut store, None, "wasi:cli/run@0.3.0")
        .unwrap();
    let index = instance
        .get_export_index(&mut store, Some(&iface), "run")
        .unwrap();
    let run = instance.get_func(&mut store, index).unwrap();
    let mut results = vec![Val::Bool(false)];
    block_on(store.run_concurrent(async |accessor| {
        run.call_concurrent(accessor, &[], &mut results)
            .await
            .unwrap();
    }))
    .unwrap();
    let out = String::from_utf8(stdout.contents().to_vec()).unwrap();
    (results.remove(0), out)
}

/// The target program of 0.4 for WASI 0.3 (§ 15.7).
const HELLO: &str = include_str!("../../examples/hello/src/hello.wasl");

#[test]
fn hello() {
    let (result, out) = run_command(&build(HELLO, "hello"), "");
    assert_eq!(result, Val::Result(Ok(None)));
    assert_eq!(out, "Hello, world!\n");
}

#[test]
fn hello_wit() {
    // The WIT of § 15.7.
    let package = Package::new("example", "hello", "0.1.0").unwrap();
    let wit = waslc::analyze(&package, &sources(HELLO), &deps())
        .unwrap()
        .wit()
        .unwrap();
    let spec = include_str!("../../docs/spec_v0.5/15-examples.md");
    let section = &spec[spec.find("## 15.7").unwrap()..];
    let start = section.find("package example:hello@0.1.0;").unwrap();
    let end = start + section[start..].find("```").unwrap();
    let lines = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect()
    };
    assert_eq!(lines(&wit), lines(&section[start..end]));
}

/// Copies the standard input to the standard output, four bytes at a time: reads until
/// the end of the stream, which `stream.read` reports again after it.
const ECHO: &str = r#"
use wasi:cli/types;
use wasi:cli/stdin;
use wasi:cli/stdout;

world echo {
  import wasi:cli/types;
  import wasi:cli/stdin;
  import wasi:cli/stdout;

  export wasi:cli/run {
    async func run() -> result {
      local (input: stream<u8>, read_done: future<result<_, types::error_code>>) = stdin::read_via_stream();
      local (rx: stream<u8>, tx: stream_writer<u8>) = stream.new();
      local write_done: future<result<_, types::error_code>> = stdout::write_via_stream(rx);
      local total: u32 = 0;
      local reading: bool = true;
      while (reading) {
        match (stream.read(input, 4)) {
          case some(bytes) {
            if (list.len(bytes) > 4) {
              return result.err;
            }
            total = total + stream.write(tx, bytes);
          }
          case none {
            reading = false;
          }
        }
      }
      match (stream.read(input, 4)) {
        case some {
          return result.err;
        }
        case none { }
      }
      stream.write(tx, text::digits(total));
      stream.close(tx);
      match (future.read(read_done)) {
        case ok { }
        case err {
          return result.err;
        }
      }
      match (future.read(write_done)) {
        case ok {
          return result.ok;
        }
        case err {
          return result.err;
        }
      }
    }
  }
}

interface text {
  func digits(n: u32) -> list<u8> {
    if (n < 10) {
      return [u8(n) + 48];
    }
    return list.freeze(text::concat(text::digits(n / 10), [u8(n % 10) + 48]));
  }

  func concat(a: list<u8>, b: list<u8>) -> list<mut u8> {
    local out: list<mut u8> = list.filled(list.len(a) + list.len(b), 0);
    local i: u32 = 0;
    while (i < list.len(a)) {
      out[i] = a[i];
      i = i + 1;
    }
    while (i < list.len(out)) {
      out[i] = b[i - list.len(a)];
      i = i + 1;
    }
    return out;
  }
}
"#;

#[test]
fn echo() {
    let bytes = build(ECHO, "echo");
    let (result, out) = run_command(&bytes, "streams and futures\n");
    assert_eq!(result, Val::Result(Ok(None)));
    assert_eq!(out, "streams and futures\n20");
    let (result, out) = run_command(&bytes, "");
    assert_eq!(result, Val::Result(Ok(None)));
    assert_eq!(out, "0");
}

/// A host that takes the readable ends of futures and streams from the component.
const SINK: &str = r#"
package demo:host@1.0.0;

interface sink {
  take: func(f: future<u32>);
  taken: async func() -> u32;
  refuse: func(f: future<u32>);
  refuse-stream: func(s: stream<u8>);
}

interface run {
  run: async func() -> result;
}
"#;

/// A host reader of a future: pending once, so that the writer waits, then takes the
/// value.
struct Taker {
    slot: Arc<Mutex<Option<u32>>>,
    pending: bool,
}

impl<D> FutureConsumer<D> for Taker {
    type Item = u32;

    fn poll_consume(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        store: StoreContextMut<D>,
        mut source: Source<'_, u32>,
        _finish: bool,
    ) -> Poll<wasmtime::Result<()>> {
        if std::mem::take(&mut self.pending) {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let mut value = None;
        source.read(store, &mut value)?;
        *self.slot.lock().unwrap() = value;
        Poll::Ready(Ok(()))
    }
}

/// Runs `demo:host/run` of world `w` of `source` against the host of [`SINK`].
fn run_sink(source: &str) -> wasmtime::Result<Val> {
    let bytes = {
        let mut deps = deps();
        deps.add_wit("demo:host", "host.wit", SINK).unwrap();
        let package = Package::new("test", "app", "0.1.0").unwrap();
        waslc::analyze(&package, &sources(source), &deps)
            .map_err(|mut e| e.remove(0))
            .and_then(|analysis| analysis.build("w"))
            .unwrap_or_else(|e| panic!("{}", e.display(&sources(source))))
    };
    let mut config = wasmtime::Config::new();
    config.wasm_gc(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::new(&engine, &bytes).unwrap();
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    let slot = Arc::new(Mutex::new(None));
    let mut sink = linker.instance("demo:host/sink@1.0.0").unwrap();
    let taken = slot.clone();
    sink.func_wrap(
        "take",
        move |mut store: StoreContextMut<'_, ()>, (f,): (FutureReader<u32>,)| {
            f.pipe(
                &mut store,
                Taker {
                    slot: taken.clone(),
                    pending: true,
                },
            )
        },
    )
    .unwrap();
    let taken = slot.clone();
    sink.func_new_concurrent("taken", move |_, _, _, results| {
        let taken = taken.clone();
        Box::pin(async move {
            let value = poll_fn(|cx| match *taken.lock().unwrap() {
                Some(v) => Poll::Ready(v),
                None => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;
            results[0] = Val::U32(value);
            Ok(())
        })
    })
    .unwrap();
    sink.func_wrap(
        "refuse",
        |mut store: StoreContextMut<'_, ()>, (mut f,): (FutureReader<u32>,)| f.close(&mut store),
    )
    .unwrap();
    sink.func_wrap(
        "refuse-stream",
        |mut store: StoreContextMut<'_, ()>, (mut s,): (StreamReader<u8>,)| s.close(&mut store),
    )
    .unwrap();
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let iface = instance
        .get_export_index(&mut store, None, "demo:host/run@1.0.0")
        .unwrap();
    let index = instance
        .get_export_index(&mut store, Some(&iface), "run")
        .unwrap();
    let run = instance.get_func(&mut store, index).unwrap();
    let mut results = vec![Val::Bool(false)];
    block_on(
        store.run_concurrent(async |accessor| {
            run.call_concurrent(accessor, &[], &mut results).await
        }),
    )??;
    Ok(results.remove(0))
}

#[test]
fn future_write() {
    // The host takes the value after a wait.
    let result = run_sink(
        r#"
use demo:host/sink;

        world w {
          import demo:host/sink;
          export demo:host/run {
            async func run() -> result {
              local (rx: future<u32>, tx: future_writer<u32>) = future.new();
              sink::take(rx);
              if (!future.write(tx, 7)) {
                return result.err;
              }
              if (sink::taken() != 7) {
                return result.err;
              }
              return result.ok;
            }
          }
        }"#,
    );
    assert_eq!(result.unwrap(), Val::Result(Ok(None)));
}

#[test]
fn refused() {
    // The host drops the readable ends: `future.write` returns `false`, and the writes
    // to the stream return `0`, again and again.
    let result = run_sink(
        r#"
use demo:host/sink;

        world w {
          import demo:host/sink;
          export demo:host/run {
            async func run() -> result {
              local (rx: future<u32>, tx: future_writer<u32>) = future.new();
              sink::refuse(rx);
              if (future.write(tx, 7)) {
                return result.err;
              }
              local (s: stream<u8>, w: stream_writer<u8>) = stream.new();
              sink::refuse_stream(s);
              if (stream.write(w, [1, 2, 3]) != 0) {
                return result.err;
              }
              if (stream.write(w, [4]) != 0) {
                return result.err;
              }
              stream.close(w);
              return result.ok;
            }
          }
        }"#,
    );
    assert_eq!(result.unwrap(), Val::Result(Ok(None)));
}

#[test]
fn moved_ends() {
    // A moved end is the empty handle: releasing it again does nothing (§ 9.9)…
    let closed_twice = r#"
use demo:host/sink;

        world w {
          import demo:host/sink;
          export demo:host/run {
            async func run() -> result {
              local (s: stream<u8>, w: stream_writer<u8>) = stream.new();
              sink::refuse_stream(s);
              stream.close(w);
              stream.close(w);
              return result.ok;
            }
          }
        }"#;
    assert_eq!(run_sink(closed_twice).unwrap(), Val::Result(Ok(None)));
    // … and using it traps.
    let written_after_close = closed_twice.replace(
        "stream.close(w);\n              stream.close(w);",
        "stream.close(w);\n              stream.write(w, [1]);",
    );
    assert_ne!(written_after_close, closed_twice);
    assert!(run_sink(&written_after_close).is_err());
}

/// A world of `wasi:cli` around the body of an async `run`, and the functions of
/// interface `t`.
fn program(body: &str, funcs: &str) -> String {
    format!(
        "world w {{
           import wasi:cli/types;
           import wasi:cli/stdout;
           export wasi:cli/run {{
             async func run() -> result {{
               {body}
               return result.ok;
             }}
           }}
         }}
         interface t {{
           {funcs}
         }}"
    )
}

#[test]
fn errors() {
    let new = "local (rx: stream<u8>, tx: stream_writer<u8>) = stream.new();";
    // E0223: no function of the world carries a `stream<string>`.
    fails(
        &program(
            "local (rx: stream<string>, tx: stream_writer<string>) = stream.new();",
            "",
        ),
        Code::UncarriedEnd,
        "no function of world `w` takes or returns a `stream<string>`",
    );
    // E0220: a tuple of ends is destructured at once.
    fails(
        &program(
            "local p: tuple<stream<u8>, stream_writer<u8>> = stream.new();",
            "",
        ),
        Code::HandlePosition,
        "destructure the tuple at once, one local per handle: `local (v0: stream<u8>, v1: stream_writer<u8>) = …;`",
    );
    // E0220: the elements of a stream are copied as a list.
    fails(
        &program("", "func f(s: stream<stream<u8>>) { }"),
        Code::HandlePosition,
        "the elements of a stream",
    );
    // E0408: the type of the ends comes from the context.
    fails(
        &program("stream.new();", ""),
        Code::CannotInfer,
        "cannot infer the type of `stream.new()`",
    );
    fails(
        &program("stream.new();", ""),
        Code::CannotInfer,
        "destructure it with the types of the ends",
    );
    // E0101: a call waits by itself.
    fails(
        &program(&format!("{new} stream.write(tx, [1]).await;"), ""),
        Code::Await,
        "remove `await`",
    );
    fails(
        &program(
            "local (rx: stream<u8>, tx: stream_writer<u8>) = future.new();",
            "",
        ),
        Code::TypeMismatch,
        "`future.new()` returns a `tuple<future<T>, future_writer<T>>`",
    );
    fails(
        &program("local (body: stream<u8>) = stream.new();", ""),
        Code::TypeMismatch,
        "",
    );
    // E0400: the ends must agree.
    fails(
        &program(
            "local (rx: stream<u8>, tx: stream_writer<u32>) = stream.new();",
            "",
        ),
        Code::TypeMismatch,
        "but `stream.new()` returns a `tuple<stream<T>, stream_writer<T>>`",
    );
    fails(
        &program(&format!("{new} stream.write(rx, [1]);"), ""),
        Code::TypeMismatch,
        "`stream.write` takes a `stream_writer<T>`",
    );
    fails(
        &program(&format!("{new} stream.write(tx, [\"a\"]);"), ""),
        Code::TypeMismatch,
        "expected `list<u8>`, found `list<string>`",
    );
    // E0416: the copies wait, and only an async function waits.
    fails(
        &program(
            "",
            "func f(tx: stream_writer<u8>) -> u32 { return stream.write(tx, [1]); }",
        ),
        Code::AsyncCall,
        "`stream.write(…)` is async",
    );
    fails(
        &program(
            "",
            "func f(rx: future<u8>) -> u8 { return future.read(rx); }",
        ),
        Code::AsyncCall,
        "`future.read(…)` is async",
    );
    // E0222: a writable end never crosses the boundary.
    fails(
        "world w { export t; }
         interface t { func f(tx: stream_writer<u8>) { stream.close(tx); } }",
        Code::WriterInWit,
        "give the other component a `stream<u8>`",
    );
}

#[test]
fn unwritten_future() {
    // A `future_writer<T>` released without its value traps (§ 10.7).
    let result = run_sink(
        r#"
use demo:host/sink;

        world w {
          import demo:host/sink;
          export demo:host/run {
            async func run() -> result {
              local (rx: future<u32>, tx: future_writer<u32>) = future.new();
              sink::take(rx);
              return result.ok;
            }
          }
        }"#,
    );
    let message = format!("{:?}", result.unwrap_err());
    assert!(
        message.contains("without first writing a value"),
        "{message}"
    );
}
