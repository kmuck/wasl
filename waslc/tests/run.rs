//! Runs the produced components with wasmtime to check the semantics (specification, chapter 10).

use std::sync::OnceLock;

use wasmtime::component::{Component, Instance, Linker, Val};
use wasmtime::{Engine, Store};

use waslc::{Package, SourceMap};

const INTERFACE: &str = "test:run/t@0.1.0";

/// Test functions, exported by interface `t` of world `w`.
const PROGRAM: &str = r#"
world w {
  export t;
}

global pi: f64 = 3.14159;
global counter: mut u32 = 0;
global big: u64 = 18446744073709551615;
global neg: s8 = -128;

interface t {
  // Integers: wrapping
  func add_u8(a: u8, b: u8) -> u8 { return a + b; }
  func add_s8(a: s8, b: s8) -> s8 { return a + b; }
  func neg_s8(a: s8) -> s8 { return -a; }
  func mul_u16(a: u16, b: u16) -> u16 { return a * b; }
  func sub_u32(a: u32, b: u32) -> u32 { return a - b; }
  func add_s64(a: s64, b: s64) -> s64 { return a + b; }

  // Division and remainder
  func div_s32(a: s32, b: s32) -> s32 { return a / b; }
  func rem_s32(a: s32, b: s32) -> s32 { return a % b; }
  func div_s8(a: s8, b: s8) -> s8 { return a / b; }
  func div_u32(a: u32, b: u32) -> u32 { return a / b; }

  // Shifts and bitwise operations
  func shl_u8(a: u8, b: u8) -> u8 { return a << b; }
  func shr_s8(a: s8, b: s8) -> s8 { return a >> b; }
  func shl_u32(a: u32, b: u32) -> u32 { return a << b; }
  func shr_u32(a: u32, b: u32) -> u32 { return a >> b; }
  func bits(a: u32) -> bool { return a & 255 == 15; }

  // Comparisons
  func lt_u32(a: u32, b: u32) -> bool { return a < b; }
  func lt_s32(a: s32, b: s32) -> bool { return a < b; }
  func nan_ne(x: f64) -> bool { return x != x; }

  // Conversions
  func u8_of_u32(x: u32) -> u8 { return u8(x); }
  func s8_of_u8(x: u8) -> s8 { return s8(x); }
  func u32_of_s32(x: s32) -> u32 { return u32(x); }
  func u64_of_s8(x: s8) -> u64 { return u64(x); }
  func u16_of_s8(x: s8) -> u16 { return u16(x); }
  func u8_of_f64(x: f64) -> u8 { return u8(x); }
  func s8_of_f64(x: f64) -> s8 { return s8(x); }
  func s32_of_f64(x: f64) -> s32 { return s32(x); }
  func u32_of_f64(x: f64) -> u32 { return u32(x); }
  func s16_of_f32(x: f32) -> s16 { return s16(x); }
  func u64_of_f32(x: f32) -> u64 { return u64(x); }
  func f64_of_u64(x: u64) -> f64 { return f64(x); }
  func f64_of_s16(x: s16) -> f64 { return f64(x); }
  func f32_of_f64(x: f64) -> f32 { return f32(x); }

  // Floats
  func div_f64(a: f64, b: f64) -> f64 { return a / b; }
  func area(r: f64) -> f64 { return pi * r * r; }

  // Control flow
  func fib(n: u32) -> u64 {
    local a: u64 = 0;
    local b: u64 = 1;
    local i: u32;
    while (i < n) {
      local next: u64 = a + b;
      a = b;
      b = next;
      i = i + 1;
    }
    return a;
  }

  func fib_rec(n: u32) -> u64 {
    if (n <= 1) {
      return u64(n);
    }
    return fib_rec(n - 1) + fib_rec(n - 2);
  }

  // A local redeclared on every iteration starts again from zero.
  func rezero(n: u32) -> u32 {
    local total: u32;
    local i: u32;
    while (i < n) {
      local x: u32;
      x = x + 1;
      total = total + x;
      i = i + 1;
    }
    return total;
  }

  // break and continue only act on the innermost loop.
  func loops() -> u32 {
    local count: u32;
    local i: u32;
    while (i < 10) {
      i = i + 1;
      if (i % 2 == 0) {
        continue;
      }
      local j: u32;
      while (true) {
        j = j + 1;
        if (j == 3) {
          break;
        }
        count = count + 1;
      }
    }
    return count;
  }

  func classify(x: s32) -> u32 {
    if (x < 0) {
      return 0;
    } else if (x == 0) {
      return 1;
    } else {
      return 2;
    }
  }

  // Parameters are mutable.
  func dec(n: u32) -> u32 {
    n = n - 1;
    return n;
  }

  // Short-circuit of && and ||.
  func bump() -> bool {
    counter = counter + 1;
    return true;
  }
  func short_circuit(b: bool) -> u32 {
    counter = 0;
    if (b || bump()) { }
    if (b && bump()) { }
    return counter;
  }

  // Calls as statements, result discarded.
  func reset() { counter = 0; }
  func incr() -> u32 {
    counter = counter + 1;
    return counter;
  }
  func calls() -> u32 {
    reset();
    incr();
    incr();
    return counter;
  }

  func get_big() -> u64 { return big; }
  func get_neg() -> s8 { return neg; }
  func from_util() -> u32 { return util::seven(); }
}

interface util {
  func seven() -> u32 { return 7; }
}
"#;

fn engine_and_component() -> &'static (Engine, Component) {
    static CELL: OnceLock<(Engine, Component)> = OnceLock::new();
    CELL.get_or_init(|| {
        let package = Package::new("test", "run", "0.1.0").unwrap();
        let mut sources = SourceMap::default();
        sources.add("src/t.wasl".into(), PROGRAM.to_string());
        let bytes = waslc::analyze(&package, &sources)
            .and_then(|analysis| analysis.build("w"))
            .unwrap_or_else(|e| panic!("{}", e.display(&sources)));
        let engine = Engine::default();
        let component = Component::new(&engine, &bytes).unwrap();
        (engine, component)
    })
}

/// An instance of the test component.
struct Runner {
    store: Store<()>,
    instance: Instance,
}

impl Runner {
    fn new() -> Runner {
        let (engine, component) = engine_and_component();
        let mut store = Store::new(engine, ());
        let instance = Linker::new(engine)
            .instantiate(&mut store, component)
            .unwrap();
        Runner { store, instance }
    }

    fn try_call(&mut self, name: &str, args: &[Val]) -> wasmtime::Result<Option<Val>> {
        let iface = self
            .instance
            .get_export_index(&mut self.store, None, INTERFACE)
            .expect("exported interface");
        let index = self
            .instance
            .get_export_index(&mut self.store, Some(&iface), name)
            .unwrap_or_else(|| panic!("exported function `{name}`"));
        let func = self.instance.get_func(&mut self.store, index).unwrap();
        let mut results = vec![Val::Bool(false); func.ty(&self.store).results().len()];
        func.call(&mut self.store, args, &mut results)?;
        Ok(results.pop())
    }

    fn call(&mut self, name: &str, args: &[Val]) -> Val {
        self.try_call(name, args)
            .unwrap_or_else(|e| panic!("{name}{args:?}: {e:?}"))
            .expect("return value")
    }
}

/// Calls a function on a fresh instance (a trap makes an instance unusable).
fn call(name: &str, args: &[Val]) -> Val {
    Runner::new().call(name, args)
}

fn traps(name: &str, args: &[Val]) -> bool {
    Runner::new().try_call(name, args).is_err()
}

fn f64_result(name: &str, args: &[Val]) -> f64 {
    match call(name, args) {
        Val::Float64(v) => v,
        other => panic!("{other:?}"),
    }
}

#[test]
fn wrapping_arithmetic() {
    use Val::*;
    assert_eq!(call("add-u8", &[U8(255), U8(1)]), U8(0));
    assert_eq!(call("add-u8", &[U8(200), U8(100)]), U8(44));
    assert_eq!(call("add-s8", &[S8(127), S8(1)]), S8(-128));
    assert_eq!(call("neg-s8", &[S8(-128)]), S8(-128));
    assert_eq!(call("neg-s8", &[S8(5)]), S8(-5));
    assert_eq!(call("mul-u16", &[U16(300), U16(300)]), U16(24464));
    assert_eq!(call("sub-u32", &[U32(0), U32(1)]), U32(u32::MAX));
    assert_eq!(call("add-s64", &[S64(i64::MAX), S64(1)]), S64(i64::MIN));
}

#[test]
fn division() {
    use Val::*;
    assert_eq!(call("div-s32", &[S32(-7), S32(2)]), S32(-3));
    assert_eq!(call("rem-s32", &[S32(-7), S32(2)]), S32(-1));
    assert_eq!(call("rem-s32", &[S32(7), S32(-2)]), S32(1));
    assert_eq!(call("rem-s32", &[S32(i32::MIN), S32(-1)]), S32(0));
    assert_eq!(call("div-u32", &[U32(u32::MAX), U32(2)]), U32(u32::MAX / 2));
    assert_eq!(call("div-s8", &[S8(-128), S8(2)]), S8(-64));
    assert!(traps("div-s32", &[S32(1), S32(0)]));
    assert!(traps("rem-s32", &[S32(1), S32(0)]));
    assert!(traps("div-u32", &[U32(1), U32(0)]));
    assert!(traps("div-s32", &[S32(i32::MIN), S32(-1)]));
    assert!(traps("div-s8", &[S8(-128), S8(-1)]));
}

#[test]
fn shifts_and_bits() {
    use Val::*;
    assert_eq!(call("shl-u8", &[U8(1), U8(9)]), U8(2));
    assert_eq!(call("shl-u8", &[U8(0x81), U8(1)]), U8(2));
    assert_eq!(call("shr-s8", &[S8(-128), S8(1)]), S8(-64));
    assert_eq!(call("shl-u32", &[U32(1), U32(33)]), U32(2));
    assert_eq!(call("shr-u32", &[U32(0x8000_0000), U32(31)]), U32(1));
    assert_eq!(call("bits", &[U32(0x10F)]), Bool(true));
}

#[test]
fn comparisons() {
    use Val::*;
    assert_eq!(call("lt-u32", &[U32(1), U32(u32::MAX)]), Bool(true));
    assert_eq!(call("lt-s32", &[S32(1), S32(-1)]), Bool(false));
    assert_eq!(call("nan-ne", &[Float64(f64::NAN)]), Bool(true));
    assert_eq!(call("nan-ne", &[Float64(1.0)]), Bool(false));
}

#[test]
fn conversions() {
    use Val::*;
    assert_eq!(call("u8-of-u32", &[U32(300)]), U8(44));
    assert_eq!(call("s8-of-u8", &[U8(200)]), S8(-56));
    assert_eq!(call("u32-of-s32", &[S32(-1)]), U32(u32::MAX));
    assert_eq!(call("u64-of-s8", &[S8(-1)]), U64(u64::MAX));
    assert_eq!(call("u16-of-s8", &[S8(-1)]), U16(u16::MAX));
    assert_eq!(call("u8-of-f64", &[Float64(300.7)]), U8(255));
    assert_eq!(call("u8-of-f64", &[Float64(3.9)]), U8(3));
    assert_eq!(call("u8-of-f64", &[Float64(-1.0)]), U8(0));
    assert_eq!(call("u8-of-f64", &[Float64(f64::NAN)]), U8(0));
    assert_eq!(call("s8-of-f64", &[Float64(-1e10)]), S8(-128));
    assert_eq!(call("s8-of-f64", &[Float64(1e10)]), S8(127));
    assert_eq!(call("s32-of-f64", &[Float64(-3.9)]), S32(-3));
    assert_eq!(call("u32-of-f64", &[Float64(-1.0)]), U32(0));
    assert_eq!(call("u32-of-f64", &[Float64(1e20)]), U32(u32::MAX));
    assert_eq!(call("s16-of-f32", &[Float32(1e9)]), S16(i16::MAX));
    assert_eq!(call("u64-of-f32", &[Float32(f32::INFINITY)]), U64(u64::MAX));
    assert_eq!(
        f64_result("f64-of-u64", &[U64(u64::MAX)]),
        18446744073709551615.0
    );
    assert_eq!(f64_result("f64-of-s16", &[S16(-5)]), -5.0);
    assert_eq!(call("f32-of-f64", &[Float64(0.1)]), Float32(0.1));
}

#[test]
fn floats() {
    use Val::*;
    assert_eq!(
        f64_result("div-f64", &[Float64(1.0), Float64(0.0)]),
        f64::INFINITY
    );
    assert!(f64_result("div-f64", &[Float64(0.0), Float64(0.0)]).is_nan());
    assert!((f64_result("area", &[Float64(2.0)]) - 12.56636).abs() < 1e-9);
}

#[test]
fn control_flow() {
    use Val::*;
    assert_eq!(call("fib", &[U32(0)]), U64(0));
    assert_eq!(call("fib", &[U32(10)]), U64(55));
    assert_eq!(call("fib", &[U32(90)]), U64(2880067194370816120));
    assert_eq!(call("fib-rec", &[U32(20)]), U64(6765));
    assert_eq!(call("rezero", &[U32(5)]), U32(5));
    assert_eq!(call("loops", &[]), U32(10));
    assert_eq!(call("classify", &[S32(-5)]), U32(0));
    assert_eq!(call("classify", &[S32(0)]), U32(1));
    assert_eq!(call("classify", &[S32(9)]), U32(2));
    assert_eq!(call("dec", &[U32(10)]), U32(9));
    assert_eq!(call("from-util", &[]), U32(7));
}

#[test]
fn short_circuit() {
    use Val::*;
    // b = true: `||` does not evaluate bump(), `&&` evaluates it once.
    assert_eq!(call("short-circuit", &[Bool(true)]), U32(1));
    // b = false: `||` evaluates bump(), `&&` does not.
    assert_eq!(call("short-circuit", &[Bool(false)]), U32(1));
}

#[test]
fn globals_and_state() {
    use Val::*;
    assert_eq!(call("get-big", &[]), U64(u64::MAX));
    assert_eq!(call("get-neg", &[]), S8(-128));
    // The state of a mut global persists between calls on the same instance.
    let mut runner = Runner::new();
    assert_eq!(runner.call("calls", &[]), U32(2));
    assert_eq!(runner.call("incr", &[]), U32(3));
    assert_eq!(runner.call("incr", &[]), U32(4));
}
