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
  // Integers: the exact result, or a trap
  func add_u8(a: u8, b: u8) -> u8 { return a + b; }
  func sub_u8(a: u8, b: u8) -> u8 { return a - b; }
  func add_s8(a: s8, b: s8) -> s8 { return a + b; }
  func mul_s8(a: s8, b: s8) -> s8 { return a * b; }
  func neg_s8(a: s8) -> s8 { return -a; }
  func mul_u16(a: u16, b: u16) -> u16 { return a * b; }
  func sub_s16(a: s16, b: s16) -> s16 { return a - b; }
  func add_u32(a: u32, b: u32) -> u32 { return a + b; }
  func sub_u32(a: u32, b: u32) -> u32 { return a - b; }
  func mul_u32(a: u32, b: u32) -> u32 { return a * b; }
  func add_s32(a: s32, b: s32) -> s32 { return a + b; }
  func sub_s32(a: s32, b: s32) -> s32 { return a - b; }
  func mul_s32(a: s32, b: s32) -> s32 { return a * b; }
  func neg_s32(a: s32) -> s32 { return -a; }
  func add_u64(a: u64, b: u64) -> u64 { return a + b; }
  func sub_u64(a: u64, b: u64) -> u64 { return a - b; }
  func mul_u64(a: u64, b: u64) -> u64 { return a * b; }
  func add_s64(a: s64, b: s64) -> s64 { return a + b; }
  func sub_s64(a: s64, b: s64) -> s64 { return a - b; }
  func mul_s64(a: s64, b: s64) -> s64 { return a * b; }
  func neg_s64(a: s64) -> s64 { return -a; }

  // Explicit wrapping and checked arithmetic
  func wrapping_add_u8(a: u8, b: u8) -> u8 { return u8.wrapping_add(a, b); }
  func wrapping_sub_u32(a: u32, b: u32) -> u32 { return u32.wrapping_sub(a, b); }
  func wrapping_mul_u16(a: u16, b: u16) -> u16 { return u16.wrapping_mul(a, b); }
  func wrapping_add_s64(a: s64, b: s64) -> s64 { return s64.wrapping_add(a, b); }
  func wrapping_neg_s8(a: s8) -> s8 { return s8.wrapping_neg(a); }
  func wrapping_literals() -> u32 { return u32.wrapping_add(4294967295, 2); }
  func hash(s: string) -> u32 {
    local bytes: list<u8> = string.bytes(s);
    local h: u32 = 5381;
    local i: u32;
    while (i < list.len(bytes)) {
      h = u32.wrapping_add(u32.wrapping_mul(h, 33), u32(bytes[i]));
      i = i + 1;
    }
    return h;
  }
  func checked_add_u8(a: u8, b: u8) -> option<u8> { return u8.checked_add(a, b); }
  func checked_sub_s32(a: s32, b: s32) -> option<s32> { return s32.checked_sub(a, b); }
  func checked_mul_u64(a: u64, b: u64) -> option<u64> { return u64.checked_mul(a, b); }
  func checked_mul_s64(a: s64, b: s64) -> option<s64> { return s64.checked_mul(a, b); }

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
  func u8_wrapping_from_u32(x: u32) -> u8 { return u8.wrapping_from(x); }
  func s8_wrapping_from_u8(x: u8) -> s8 { return s8.wrapping_from(x); }
  func u32_wrapping_from_s32(x: s32) -> u32 { return u32.wrapping_from(x); }
  func u16_wrapping_from_s64(x: s64) -> u16 { return u16.wrapping_from(x); }
  func u8_of_u32(x: u32) -> u8 { return u8(x); }
  func s16_of_s64(x: s64) -> s16 { return s16(x); }
  func s32_of_u32(x: u32) -> s32 { return s32(x); }
  func u64_of_s64(x: s64) -> u64 { return u64(x); }
  func s64_of_u64(x: u64) -> s64 { return s64(x); }
  func u8_of_s8(x: s8) -> u8 { return u8(x); }
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

  // Decimal form of integers
  func u8_to_string(n: u8) -> string { return u8.to_string(n); }
  func u8_parse(s: string) -> option<u8> { return u8.parse(s); }
  func s8_to_string(n: s8) -> string { return s8.to_string(n); }
  func s8_parse(s: string) -> option<s8> { return s8.parse(s); }
  func u16_to_string(n: u16) -> string { return u16.to_string(n); }
  func u16_parse(s: string) -> option<u16> { return u16.parse(s); }
  func s16_to_string(n: s16) -> string { return s16.to_string(n); }
  func s16_parse(s: string) -> option<s16> { return s16.parse(s); }
  func u32_to_string(n: u32) -> string { return u32.to_string(n); }
  func u32_parse(s: string) -> option<u32> { return u32.parse(s); }
  func s32_to_string(n: s32) -> string { return s32.to_string(n); }
  func s32_parse(s: string) -> option<s32> { return s32.parse(s); }
  func u64_to_string(n: u64) -> string { return u64.to_string(n); }
  func u64_parse(s: string) -> option<u64> { return u64.parse(s); }
  func s64_to_string(n: s64) -> string { return s64.to_string(n); }
  func s64_parse(s: string) -> option<s64> { return s64.parse(s); }
  func literals_to_string() -> string { return s64.to_string(-9223372036854775808) + " " + u8.to_string(7); }
  func round_trip(n: s32) -> bool {
    match (s32.parse(s32.to_string(n))) {
      case some(m) { return m == n; }
      case none { return false; }
    }
  }
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
        let bytes = waslc::analyze(&package, &sources, &Default::default())
            .map_err(|mut e| e.remove(0))
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

/// Whether the call traps with `integer overflow` (§ 10.2).
fn overflows(name: &str, args: &[Val]) -> bool {
    match Runner::new().try_call(name, args) {
        Ok(v) => panic!("{name}{args:?} returned {v:?}"),
        Err(e) => {
            assert!(format!("{e:?}").contains("integer overflow"), "{e:?}");
            true
        }
    }
}

#[test]
fn exact_arithmetic() {
    use Val::*;
    // The largest results that fit, and the smallest that do not.
    assert_eq!(call("add-u8", &[U8(200), U8(55)]), U8(255));
    assert!(overflows("add-u8", &[U8(255), U8(1)]));
    assert!(overflows("add-u8", &[U8(200), U8(100)]));
    assert_eq!(call("sub-u8", &[U8(5), U8(5)]), U8(0));
    assert!(overflows("sub-u8", &[U8(0), U8(1)]));
    assert_eq!(call("add-s8", &[S8(-100), S8(-28)]), S8(-128));
    assert!(overflows("add-s8", &[S8(127), S8(1)]));
    assert!(overflows("add-s8", &[S8(-128), S8(-1)]));
    assert_eq!(call("mul-s8", &[S8(-16), S8(8)]), S8(-128));
    assert!(overflows("mul-s8", &[S8(16), S8(8)]));
    assert_eq!(call("neg-s8", &[S8(5)]), S8(-5));
    assert_eq!(call("neg-s8", &[S8(127)]), S8(-127));
    assert!(overflows("neg-s8", &[S8(-128)]));
    assert_eq!(call("mul-u16", &[U16(255), U16(257)]), U16(65535));
    assert!(overflows("mul-u16", &[U16(300), U16(300)]));
    assert!(overflows("mul-u16", &[U16(65535), U16(65535)]));
    assert!(overflows("sub-s16", &[S16(-32768), S16(1)]));
    assert_eq!(call("add-u32", &[U32(u32::MAX - 1), U32(1)]), U32(u32::MAX));
    assert!(overflows("add-u32", &[U32(u32::MAX), U32(1)]));
    assert!(overflows("sub-u32", &[U32(0), U32(1)]));
    assert_eq!(call("mul-u32", &[U32(65535), U32(65537)]), U32(u32::MAX));
    assert!(overflows("mul-u32", &[U32(65536), U32(65536)]));
    assert_eq!(
        call("add-s32", &[S32(-1), S32(i32::MIN + 1)]),
        S32(i32::MIN)
    );
    assert!(overflows("add-s32", &[S32(i32::MAX), S32(1)]));
    assert_eq!(call("sub-s32", &[S32(-1), S32(i32::MAX)]), S32(i32::MIN));
    assert!(overflows("sub-s32", &[S32(i32::MIN), S32(1)]));
    assert!(overflows("sub-s32", &[S32(0), S32(i32::MIN)]));
    assert_eq!(call("mul-s32", &[S32(-65536), S32(32768)]), S32(i32::MIN));
    assert!(overflows("mul-s32", &[S32(65536), S32(32768)]));
    assert!(overflows("mul-s32", &[S32(-1), S32(i32::MIN)]));
    assert_eq!(call("neg-s32", &[S32(i32::MAX)]), S32(-i32::MAX));
    assert!(overflows("neg-s32", &[S32(i32::MIN)]));
    assert!(overflows("add-u64", &[U64(u64::MAX), U64(1)]));
    assert!(overflows("sub-u64", &[U64(1), U64(2)]));
    assert!(overflows("add-s64", &[S64(i64::MAX), S64(1)]));
    assert_eq!(call("sub-s64", &[S64(-1), S64(i64::MAX)]), S64(i64::MIN));
    assert!(overflows("sub-s64", &[S64(i64::MIN), S64(1)]));
    assert!(overflows("neg-s64", &[S64(i64::MIN)]));
    // 64-bit products: the fast path (both operands in 32 bits) and the division check.
    assert_eq!(
        call("mul-u64", &[U64(u32::MAX as u64), U64(u32::MAX as u64)]),
        U64(u32::MAX as u64 * u32::MAX as u64)
    );
    assert_eq!(call("mul-u64", &[U64(1 << 40), U64(1 << 23)]), U64(1 << 63));
    assert_eq!(call("mul-u64", &[U64(0), U64(u64::MAX)]), U64(0));
    assert_eq!(call("mul-u64", &[U64(u64::MAX), U64(1)]), U64(u64::MAX));
    assert!(overflows("mul-u64", &[U64(1 << 40), U64(1 << 24)]));
    assert!(overflows("mul-u64", &[U64(u64::MAX), U64(2)]));
    assert_eq!(
        call("mul-s64", &[S64(i32::MIN as i64), S64(i32::MIN as i64)]),
        S64(1 << 62)
    );
    assert_eq!(
        call("mul-s64", &[S64(-(1 << 40)), S64(1 << 23)]),
        S64(i64::MIN)
    );
    assert_eq!(call("mul-s64", &[S64(-1), S64(i64::MAX)]), S64(-i64::MAX));
    assert_eq!(call("mul-s64", &[S64(i64::MIN), S64(1)]), S64(i64::MIN));
    assert!(overflows("mul-s64", &[S64(1 << 40), S64(1 << 23)]));
    assert!(overflows("mul-s64", &[S64(-1), S64(i64::MIN)]));
    assert!(overflows("mul-s64", &[S64(i64::MIN), S64(-1)]));
    assert!(overflows("mul-s64", &[S64(i64::MIN), S64(2)]));
    // `fib(n)` also computes fib(n + 1), which no longer fits in a u64 for n = 93.
    assert_eq!(call("fib", &[U32(92)]), U64(7540113804746346429));
    assert!(overflows("fib", &[U32(93)]));
}

#[test]
fn wrapping_and_checked_arithmetic() {
    use Val::*;
    assert_eq!(call("wrapping-add-u8", &[U8(200), U8(100)]), U8(44));
    assert_eq!(call("wrapping-sub-u32", &[U32(0), U32(1)]), U32(u32::MAX));
    assert_eq!(call("wrapping-mul-u16", &[U16(300), U16(300)]), U16(24464));
    assert_eq!(
        call("wrapping-add-s64", &[S64(i64::MAX), S64(1)]),
        S64(i64::MIN)
    );
    assert_eq!(call("wrapping-neg-s8", &[S8(-128)]), S8(-128));
    assert_eq!(call("wrapping-literals", &[]), U32(1));
    // djb2 of "hello world", which wraps.
    let mut h: u32 = 5381;
    for b in b"hello world" {
        h = h.wrapping_mul(33).wrapping_add(*b as u32);
    }
    assert_eq!(call("hash", &[String("hello world".into())]), U32(h));

    let some = |v: Val| Option(Some(Box::new(v)));
    assert_eq!(call("checked-add-u8", &[U8(200), U8(55)]), some(U8(255)));
    assert_eq!(call("checked-add-u8", &[U8(200), U8(56)]), Option(None));
    assert_eq!(
        call("checked-sub-s32", &[S32(-1), S32(i32::MAX)]),
        some(S32(i32::MIN))
    );
    assert_eq!(
        call("checked-sub-s32", &[S32(i32::MIN), S32(1)]),
        Option(None)
    );
    assert_eq!(
        call("checked-mul-u64", &[U64(1 << 40), U64(1 << 23)]),
        some(U64(1 << 63))
    );
    assert_eq!(
        call("checked-mul-u64", &[U64(1 << 40), U64(1 << 24)]),
        Option(None)
    );
    assert_eq!(
        call("checked-mul-s64", &[S64(i64::MIN), S64(-1)]),
        Option(None)
    );
    assert_eq!(
        call("checked-mul-s64", &[S64(-1), S64(i64::MIN)]),
        Option(None)
    );
    assert_eq!(call("checked-mul-s64", &[S64(-1), S64(5)]), some(S64(-5)));
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

/// Whether the call traps with `invalid conversion to integer` (NaN).
fn bad_conversion(name: &str, args: &[Val]) -> bool {
    match Runner::new().try_call(name, args) {
        Ok(v) => panic!("{name}{args:?} returned {v:?}"),
        Err(e) => {
            assert!(
                format!("{e:?}").contains("invalid conversion to integer"),
                "{e:?}"
            );
            true
        }
    }
}

#[test]
fn conversions() {
    use Val::*;
    // Integer to integer: the same value, or a trap.
    assert_eq!(call("u8-of-u32", &[U32(255)]), U8(255));
    assert!(overflows("u8-of-u32", &[U32(256)]));
    assert_eq!(call("s8-of-u8", &[U8(127)]), S8(127));
    assert!(overflows("s8-of-u8", &[U8(200)]));
    assert_eq!(call("u8-of-s8", &[S8(127)]), U8(127));
    assert!(overflows("u8-of-s8", &[S8(-1)]));
    assert_eq!(call("u32-of-s32", &[S32(i32::MAX)]), U32(i32::MAX as u32));
    assert!(overflows("u32-of-s32", &[S32(-1)]));
    assert_eq!(call("s32-of-u32", &[U32(i32::MAX as u32)]), S32(i32::MAX));
    assert!(overflows("s32-of-u32", &[U32(1 << 31)]));
    assert!(overflows("u64-of-s8", &[S8(-1)]));
    assert_eq!(call("u64-of-s8", &[S8(127)]), U64(127));
    assert!(overflows("u16-of-s8", &[S8(-1)]));
    assert_eq!(call("s16-of-s64", &[S64(-32768)]), S16(-32768));
    assert!(overflows("s16-of-s64", &[S64(-32769)]));
    assert!(overflows("s16-of-s64", &[S64(32768)]));
    assert!(overflows("s16-of-s64", &[S64(i64::MIN)]));
    assert!(overflows("u64-of-s64", &[S64(-1)]));
    assert_eq!(call("u64-of-s64", &[S64(i64::MAX)]), U64(i64::MAX as u64));
    assert!(overflows("s64-of-u64", &[U64(1 << 63)]));
    // `T::wrapping_from` keeps the low-order bits.
    assert_eq!(call("u8-wrapping-from-u32", &[U32(300)]), U8(44));
    assert_eq!(call("s8-wrapping-from-u8", &[U8(200)]), S8(-56));
    assert_eq!(call("u32-wrapping-from-s32", &[S32(-1)]), U32(u32::MAX));
    assert_eq!(call("u16-wrapping-from-s64", &[S64(-1)]), U16(u16::MAX));
    // Float to integer: truncated toward zero, a trap if NaN or out of range.
    assert_eq!(call("u8-of-f64", &[Float64(3.9)]), U8(3));
    assert_eq!(call("u8-of-f64", &[Float64(255.9)]), U8(255));
    assert_eq!(call("u8-of-f64", &[Float64(-0.9)]), U8(0));
    assert!(overflows("u8-of-f64", &[Float64(256.0)]));
    assert!(overflows("u8-of-f64", &[Float64(-1.0)]));
    assert!(bad_conversion("u8-of-f64", &[Float64(f64::NAN)]));
    assert_eq!(call("s8-of-f64", &[Float64(-128.5)]), S8(-128));
    assert!(overflows("s8-of-f64", &[Float64(-129.0)]));
    assert!(overflows("s8-of-f64", &[Float64(1e10)]));
    assert_eq!(call("s32-of-f64", &[Float64(-3.9)]), S32(-3));
    assert!(overflows("s32-of-f64", &[Float64(3e9)]));
    assert!(overflows("u32-of-f64", &[Float64(-1.0)]));
    assert!(overflows("u32-of-f64", &[Float64(1e20)]));
    assert_eq!(call("u32-of-f64", &[Float64(4294967295.0)]), U32(u32::MAX));
    assert!(overflows("s16-of-f32", &[Float32(1e9)]));
    assert!(overflows("u64-of-f32", &[Float32(f32::INFINITY)]));
    assert!(bad_conversion("u64-of-f32", &[Float32(f32::NAN)]));
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

#[test]
fn integers_to_string() {
    let text = |name: &str, arg: Val| match call(name, &[arg]) {
        Val::String(s) => s,
        other => panic!("{other:?}"),
    };
    for n in [0u8, 7, 10, 99, 100, 255] {
        assert_eq!(text("u8-to-string", Val::U8(n)), n.to_string());
    }
    for n in [0i8, 1, -1, 127, -128] {
        assert_eq!(text("s8-to-string", Val::S8(n)), n.to_string());
    }
    for n in [0u16, 65535] {
        assert_eq!(text("u16-to-string", Val::U16(n)), n.to_string());
    }
    for n in [-32768i16, 32767, -10] {
        assert_eq!(text("s16-to-string", Val::S16(n)), n.to_string());
    }
    for n in [0u32, 1205, u32::MAX] {
        assert_eq!(text("u32-to-string", Val::U32(n)), n.to_string());
    }
    for n in [0i32, -1, i32::MIN, i32::MAX] {
        assert_eq!(text("s32-to-string", Val::S32(n)), n.to_string());
    }
    for n in [0u64, 10_000_000_000, u64::MAX] {
        assert_eq!(text("u64-to-string", Val::U64(n)), n.to_string());
    }
    for n in [0i64, -1, i64::MIN, i64::MAX] {
        assert_eq!(text("s64-to-string", Val::S64(n)), n.to_string());
    }
    assert_eq!(
        call("literals-to-string", &[]),
        Val::String("-9223372036854775808 7".into())
    );
    for n in [0, -1, 42, i32::MIN, i32::MAX] {
        assert_eq!(call("round-trip", &[Val::S32(n)]), Val::Bool(true));
    }
}

#[test]
fn integers_parse() {
    let parse = |name: &str, text: &str| match call(name, &[Val::String(text.into())]) {
        Val::Option(v) => v.map(|v| *v),
        other => panic!("{other:?}"),
    };
    assert_eq!(parse("u8-parse", "0"), Some(Val::U8(0)));
    assert_eq!(parse("u8-parse", "255"), Some(Val::U8(255)));
    assert_eq!(parse("u8-parse", "256"), None);
    assert_eq!(parse("u8-parse", "-1"), None);
    assert_eq!(parse("s8-parse", "-128"), Some(Val::S8(-128)));
    assert_eq!(parse("s8-parse", "127"), Some(Val::S8(127)));
    assert_eq!(parse("s8-parse", "128"), None);
    assert_eq!(parse("s8-parse", "-129"), None);
    assert_eq!(parse("u16-parse", "65535"), Some(Val::U16(65535)));
    assert_eq!(parse("u16-parse", "65536"), None);
    assert_eq!(parse("s16-parse", "-32768"), Some(Val::S16(-32768)));
    assert_eq!(parse("s16-parse", "32768"), None);
    assert_eq!(parse("u32-parse", "4294967295"), Some(Val::U32(u32::MAX)));
    assert_eq!(parse("u32-parse", "4294967296"), None);
    assert_eq!(parse("s32-parse", "-2147483648"), Some(Val::S32(i32::MIN)));
    assert_eq!(parse("s32-parse", "2147483648"), None);
    assert_eq!(
        parse("u64-parse", "18446744073709551615"),
        Some(Val::U64(u64::MAX))
    );
    assert_eq!(parse("u64-parse", "18446744073709551616"), None);
    assert_eq!(parse("u64-parse", "99999999999999999999999"), None);
    assert_eq!(
        parse("s64-parse", "-9223372036854775808"),
        Some(Val::S64(i64::MIN))
    );
    assert_eq!(
        parse("s64-parse", "9223372036854775807"),
        Some(Val::S64(i64::MAX))
    );
    assert_eq!(parse("s64-parse", "9223372036854775808"), None);
    assert_eq!(parse("s64-parse", "-9223372036854775809"), None);
    // Only what `to_string` writes: no sign `+`, leading zero, space or other character.
    for text in [
        "", "-", "+1", "01", "00", "-0", "1 ", " 1", "1.0", "1_000", "0x10", "١",
    ] {
        assert_eq!(parse("s32-parse", text), None, "{text:?}");
        assert_eq!(parse("u64-parse", text), None, "{text:?}");
    }
    assert_eq!(parse("s32-parse", "0"), Some(Val::S32(0)));
    assert_eq!(parse("s32-parse", "-7"), Some(Val::S32(-7)));
}
