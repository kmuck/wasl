//! Async functions (specification, § 2.8, § 13.13): `async func`, the colors of calls
//! (E0416), and async exports lifted with a callback, run by wasmtime with async host
//! functions.

use std::future::{Future, poll_fn};
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};

use wasmtime::component::{Component, Func, Instance, Linker, Val};
use wasmtime::{Engine, Store};

use waslc::{Dependencies, Package, SourceMap, diag::Code};

/// The WIT of the dependency `demo:host`, provided by the host in these tests.
const HOST: &str = r#"
package demo:host@1.0.0;

interface clock {
  sleep: async func(ms: u32) -> u32;
  greet: async func(name: string) -> string;
  pair: async func() -> tuple<u32, string>;
  now: func() -> u64;
}

interface run {
  run: async func() -> result;
}
"#;

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

/// Runs a future to its end, polling it again and again: the host functions of these
/// tests wake their task themselves.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

/// A future that is pending until `ready` is set, and asks to be polled again
/// meanwhile.
async fn until(ready: Arc<AtomicBool>) {
    poll_fn(|cx| {
        if ready.load(Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}

/// The host side of `demo:host/clock`. Its async functions are pending once before
/// they deliver their result, so the task that calls them really waits; `greet` waits
/// until `released` is set.
fn host(linker: &mut Linker<()>, released: Arc<AtomicBool>) {
    let mut clock = linker.instance("demo:host/clock@1.0.0").unwrap();
    clock
        .func_new_concurrent("sleep", |_, _, params, results| {
            Box::pin(async move {
                let Val::U32(ms) = params[0] else { panic!() };
                let mut first = true;
                poll_fn(|cx| {
                    if std::mem::take(&mut first) {
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    } else {
                        Poll::Ready(())
                    }
                })
                .await;
                results[0] = Val::U32(ms + 1000);
                Ok(())
            })
        })
        .unwrap();
    clock
        .func_new_concurrent("greet", move |_, _, params, results| {
            let released = released.clone();
            Box::pin(async move {
                let Val::String(name) = &params[0] else {
                    panic!()
                };
                until(released).await;
                // Long enough to cover the return area if the scratch buffer were
                // given back too early (§ 13.13).
                results[0] = Val::String(format!("hello, {name}! {}", "-".repeat(64)));
                Ok(())
            })
        })
        .unwrap();
    clock
        .func_new_concurrent("pair", |_, _, _, results| {
            Box::pin(async move {
                results[0] = Val::Tuple(vec![Val::U32(7), Val::String("seven".into())]);
                Ok(())
            })
        })
        .unwrap();
    clock
        .func_new("now", |_, _, _, results| {
            results[0] = Val::U64(42);
            Ok(())
        })
        .unwrap();
}

struct Runner {
    store: Store<()>,
    instance: Instance,
    /// Lets `clock::greet` deliver its result.
    released: Arc<AtomicBool>,
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
        let released = Arc::new(AtomicBool::new(true));
        host(&mut linker, released.clone());
        let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
        Runner {
            store,
            instance,
            released,
        }
    }

    fn func(&mut self, name: &str) -> Func {
        let iface = self
            .instance
            .get_export_index(&mut self.store, None, "test:app/t@0.1.0")
            .unwrap();
        let index = self
            .instance
            .get_export_index(&mut self.store, Some(&iface), name)
            .unwrap_or_else(|| panic!("function `{name}`"));
        self.instance.get_func(&mut self.store, index).unwrap()
    }

    fn call(&mut self, name: &str, args: &[Val]) -> Option<Val> {
        let func = self.func(name);
        let mut results = vec![Val::Bool(false); func.ty(&self.store).results().len()];
        block_on(func.call_async(&mut self.store, args, &mut results))
            .unwrap_or_else(|e| panic!("{name}{args:?}: {e:?}"));
        results.pop()
    }
}

fn string(s: &str) -> Val {
    Val::String(s.into())
}

const PROGRAM: &str = r#"
use demo:host/clock;

world w {
  import demo:host/clock;
  export t;
}

interface t {
  record big {
    a: u32, b: u32, c: u32, d: u32, e: u32, f: u32, g: u32, h: u32, i: u32,
    j: u32, k: u32, l: u32, m: u32, n: u32, o: u32, p: u32, q: u32,
  }

  // Scalars: `task.return` takes the result as flat values.
  async func add(a: u32, b: u32) -> u32 {
    return clock::sleep(a) + b;
  }

  // Strings in both directions, and an import that returns through memory.
  async func greet(name: string) -> string {
    return clock::greet(name);
  }

  // An async import that returns a tuple, then a sync import.
  async func pair() -> tuple<u32, string, u64> {
    local (n: u32, s: string) = clock::pair();
    return tuple(n, s, clock::now());
  }

  // Calls to async functions of the package.
  async func lengths(name: string) -> u32 {
    return string.len(greet(name)) + add(1, 2);
  }

  // No result.
  async func tick() {
    clock::sleep(1);
  }

  // A list result.
  async func countdown(n: u32) -> list<u32> {
    local xs: list<mut u32> = list.filled(n, 0);
    local i: u32 = 0;
    while (i < n) {
      xs[i] = clock::sleep(n - i) - 1000;
      i = i + 1;
    }
    return list.freeze(xs);
  }

  // 17 flat values: `task.return` takes a pointer.
  async func wide(x: u32) -> big {
    local y: u32 = clock::sleep(x);
    return big {
      a: x, b: y, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9,
      j: 10, k: 11, l: 12, m: 13, n: 14, o: 15, p: 16, q: 17,
    };
  }

  // A sync function next to the async ones.
  func echo(s: string) -> string {
    return s;
  }

  // Async methods and static functions of an exported resource.
  resource counter {
    n: u32,
    constructor(n: u32) { return counter { n: n }; }
    async func plus(x: u32) -> u32 { return self.n + clock::sleep(x); }
    static async func make(n: u32) -> counter { return counter { n: clock::sleep(n) }; }
  }
}

// Another world imports `t` and calls its async functions.
world client {
  import t;
  export u;
}

interface u {
  async func twice(x: u32) -> u32 {
    return t::add(x, 0) + t::add(x, 0);
  }
}
"#;

#[test]
fn async_exports() {
    let mut r = Runner::new(PROGRAM, "w");
    assert_eq!(
        r.call("add", &[Val::U32(1), Val::U32(2)]),
        Some(Val::U32(1003))
    );
    let long = format!("hello, Ada! {}", "-".repeat(64));
    assert_eq!(r.call("greet", &[string("Ada")]), Some(string(&long)));
    assert_eq!(
        r.call("pair", &[]),
        Some(Val::Tuple(vec![Val::U32(7), string("seven"), Val::U64(42)]))
    );
    assert_eq!(
        r.call("lengths", &[string("Ada")]),
        Some(Val::U32(long.len() as u32 + 1003))
    );
    assert_eq!(r.call("tick", &[]), None);
    assert_eq!(
        r.call("countdown", &[Val::U32(3)]),
        Some(Val::List(vec![Val::U32(3), Val::U32(2), Val::U32(1)]))
    );
    let Some(Val::Record(fields)) = r.call("wide", &[Val::U32(5)]) else {
        panic!()
    };
    let values: Vec<&Val> = fields.iter().map(|(_, v)| v).collect();
    assert_eq!(values[0], &Val::U32(5));
    assert_eq!(values[1], &Val::U32(1005));
    assert_eq!(values[16], &Val::U32(17));
    assert_eq!(r.call("echo", &[string("sync")]), Some(string("sync")));
    let Some(Val::Resource(c)) = r.call("[constructor]counter", &[Val::U32(5)]) else {
        panic!()
    };
    assert_eq!(
        r.call("[method]counter.plus", &[Val::Resource(c), Val::U32(1)]),
        Some(Val::U32(1006))
    );
    let Some(Val::Resource(m)) = r.call("[static]counter.make", &[Val::U32(2)]) else {
        panic!()
    };
    assert_eq!(
        r.call("[method]counter.plus", &[Val::Resource(m), Val::U32(0)]),
        Some(Val::U32(2002))
    );
    // Many calls in a row: each task gives its memory back.
    for k in 0..100 {
        let name = format!("n{k}");
        let expected = format!("hello, {name}! {}", "-".repeat(64));
        assert_eq!(r.call("greet", &[string(&name)]), Some(string(&expected)));
    }
}

/// A sync call runs while an async task waits in an import: it neither disturbs nor
/// frees the data of the waiting task (§ 13.13).
#[test]
fn sync_call_while_async_task_waits() {
    let mut r = Runner::new(PROGRAM, "w");
    let (greet, echo) = (r.func("greet"), r.func("echo"));
    r.released.store(false, Ordering::SeqCst);
    let released = r.released.clone();
    let (greeting, echoed) = block_on(r.store.run_concurrent(async move |accessor| {
        let greeting = async {
            let mut results = vec![Val::Bool(false)];
            greet
                .call_concurrent(accessor, &[string("Ada")], &mut results)
                .await
                .unwrap();
            results.pop().unwrap()
        };
        let echoed = async {
            let mut results = vec![Val::Bool(false)];
            let long = "x".repeat(100);
            echo.call_concurrent(accessor, &[string(&long)], &mut results)
                .await
                .unwrap();
            released.store(true, Ordering::SeqCst);
            results.pop().unwrap()
        };
        join(greeting, echoed).await
    }))
    .unwrap();
    assert_eq!(echoed, string(&"x".repeat(100)));
    assert_eq!(greeting, string(&format!("hello, Ada! {}", "-".repeat(64))));
}

/// Both futures, run together.
async fn join<A: Future, B: Future>(a: A, b: B) -> (A::Output, B::Output) {
    let (mut a, mut b) = (pin!(a), pin!(b));
    let (mut ra, mut rb) = (None, None);
    poll_fn(|cx| {
        if ra.is_none()
            && let Poll::Ready(v) = a.as_mut().poll(cx)
        {
            ra = Some(v);
        }
        if rb.is_none()
            && let Poll::Ready(v) = b.as_mut().poll(cx)
        {
            rb = Some(v);
        }
        if ra.is_some() && rb.is_some() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    (ra.unwrap(), rb.unwrap())
}

/// The world that imports `t` builds: its async imports are lowered like the others.
#[test]
fn async_imports_of_the_package() {
    let bytes = build(PROGRAM, "client");
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&bytes)
        .unwrap();
}

#[test]
fn async_foreign_export() {
    let source = r#"
use demo:host/clock;

world w {
  import demo:host/clock;
  export demo:host/run {
    async func run() -> result {
      if (clock::sleep(1) == 1001) {
        return result.ok;
      }
      return result.err;
    }
  }
}
"#;
    let bytes = build(source, "w");
    let mut config = wasmtime::Config::new();
    config.wasm_gc(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::new(&engine, &bytes).unwrap();
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    host(&mut linker, Arc::new(AtomicBool::new(true)));
    let instance = block_on(linker.instantiate_async(&mut store, &component)).unwrap();
    let iface = instance
        .get_export_index(&mut store, None, "demo:host/run@1.0.0")
        .unwrap();
    let index = instance
        .get_export_index(&mut store, Some(&iface), "run")
        .unwrap();
    let run = instance.get_func(&mut store, index).unwrap();
    let mut results = vec![Val::Bool(false)];
    block_on(run.call_async(&mut store, &[], &mut results)).unwrap();
    assert_eq!(results[0], Val::Result(Ok(None)));
}

#[test]
fn wit() {
    let wit = analyze(
        "world w { export t; }
         interface t {
           resource counter {
             n: u32,
             constructor() { return counter { n: 0 }; }
             async func next() -> u32 { return self.n + 1; }
             static async func zero() -> u32 { return 0; }
           }
           async func f(x: u32) -> u32 { return x; }
           func g() { }
         }",
    )
    .unwrap()
    .wit()
    .unwrap();
    assert!(wit.contains("    next: async func() -> u32;"), "{wit}");
    assert!(
        wit.contains("    zero: static async func() -> u32;"),
        "{wit}"
    );
    assert!(wit.contains("  f: async func(x: u32) -> u32;"), "{wit}");
    assert!(wit.contains("  g: func();"), "{wit}");
}

#[test]
fn errors() {
    // `await` does not exist: a call waits (§ 2.8).
    fails(
        "interface t { async func f() { await g(); } async func g() { } }",
        Code::Await,
        "",
    );
    // Only an async function calls an async function (E0416).
    fails(
        "interface t { func f() { g(); } async func g() { } }",
        Code::AsyncCall,
        "`g(…)` is async, and only an async function can call it: declare this function `async func`",
    );
    fails(
        "use demo:host/clock;
world w { import demo:host/clock; export t; }
         interface t { func f() -> u32 { return clock::sleep(1); } }",
        Code::AsyncCall,
        "`clock::sleep(…)` is async",
    );
    fails(
        "interface t {
           resource counter {
             constructor() { return counter {}; }
             async func next() -> u32 { return 1; }
           }
           func f(c: borrow<counter>) -> u32 { return c.next(); }
         }",
        Code::AsyncCall,
        "`.next(…)` is async",
    );
    // Sync functions are called from anywhere.
    assert!(
        analyze(
            "use demo:host/clock;
world w { import demo:host/clock; export t; }
             interface t {
               async func f() -> u64 { return clock::now() + u64(g()); }
               func g() -> u32 { return 1; }
             }"
        )
        .is_ok()
    );
    // A constructor is never async, as in WIT.
    fails(
        "interface t { resource r { async constructor() { } } }",
        Code::Syntax,
        "a constructor is never async",
    );
    // The color of a function of a foreign export block is that of its WIT (E0218).
    fails(
        "world w { export demo:host/run { func run() -> result { return result.ok; } } }",
        Code::ForeignExport,
        "expected `async func run() -> result`, found `func run() -> result`",
    );
}
