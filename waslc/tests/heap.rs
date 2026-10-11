//! Runs code that uses the heap types of v0.2 (specification v0.2, chapters v2.5 and
//! v2.6). The heap values stay inside the component: the exported functions only take and
//! return scalars.

use std::sync::OnceLock;

use wasmtime::component::{Component, Instance, Linker, Val};
use wasmtime::{Engine, Store};

use waslc::{Package, SourceMap};

const INTERFACE: &str = "test:heap/h@0.1.0";

/// Test functions, exported by interface `h` of world `w`. `LONG` is replaced by a string
/// literal long enough to be built from a data segment.
const PROGRAM: &str = r#"
world w {
  export h;
}

global greeting: string = "héllo";
global origin: geo::point = geo::point { x: 1, y: 2 };
global none_u32: option<u32> = option::none;
global some_u32: option<u32> = option::some(7);
global shapes: list<geo::shape> = [
  geo::shape::circle(2),
  geo::shape::empty,
  geo::shape::rect(tuple(3, 4)),
];
global long_global: string = "LONG";

// Types and functions on heap values: not exported.
interface geo {
  record point {
    x: s32,
    y: mut s32,
  }

  variant shape {
    circle(u32),
    rect(tuple<u32, u32>),
    empty,
  }

  // Recursive types.
  record node {
    value: u32,
    next: option<node>,
  }

  variant tree {
    leaf(u32),
    branch(list<tree>),
  }

  // Recursive without `option` or `list`: `lit` is a value (§ v2.2.5).
  variant expr {
    lit(s32),
    neg(expr),
    add(tuple<expr, expr>),
  }

  func eval(e: expr) -> s32 {
    match (e) {
      case lit(n) {
        return n;
      }
      case neg(x) {
        return 0 - eval(x);
      }
      case add(xy) {
        return eval(xy.0) + eval(xy.1);
      }
    }
  }

  func area(s: shape) -> u32 {
    match (s) {
      case circle(r) {
        return 3 * r * r;
      }
      case rect(wh) {
        return wh.0 * wh.1;
      }
      case empty {
        return 0;
      }
    }
  }

  func chain(n: u32) -> node {
    local head: node = node { value: 0, next: option::none };
    local i: u32 = 1;
    while (i < n) {
      head = node { value: i, next: option::some(head) };
      i = i + 1;
    }
    return head;
  }

  func sum_chain(n: node) -> u32 {
    local sum: u32 = n.value;
    local cur: option<node> = n.next;
    while (true) {
      match (cur) {
        case some(m) {
          sum = sum + m.value;
          cur = m.next;
        }
        case none {
          break;
        }
      }
    }
    return sum;
  }

  func tree_sum(t: tree) -> u32 {
    match (t) {
      case leaf(v) {
        return v;
      }
      case branch(children) {
        local sum: u32;
        local i: u32;
        while (i < list::len(children)) {
          sum = sum + tree_sum(children[i]);
          i = i + 1;
        }
        return sum;
      }
    }
  }
}

interface h {
  enum color {
    red,
    green,
    blue,
  }

  flags perms {
    read,
    write,
    exec,
  }

  // ---- Strings ----

  func greeting_len() -> u32 {
    return string::len(greeting);
  }

  func str_eq(k: u32) -> bool {
    local s: string = "ab" + "c";
    if (k == 0) {
      return s == "abc";
    }
    if (k == 1) {
      return s == "abd";
    }
    if (k == 2) {
      return s != "ab";
    }
    return "" == "";
  }

  func concat_len() -> u32 {
    return string::len(greeting + ", " + "monde");
  }

  func nth_char(i: u32) -> char {
    return string::chars(greeting)[i];
  }

  func char_count() -> u32 {
    return list::len(string::chars("a€𝄞é"));
  }

  func roundtrip() -> bool {
    local s: string = "a€𝄞é\u{7f}\u{80}\u{7ff}\u{800}\u{ffff}\u{10000}\u{10ffff}";
    return string::from_chars(string::chars(s)) == s;
  }

  func bytes_sum() -> u32 {
    local b: list<u8> = string::bytes("AB");
    return u32(b[0]) + u32(b[1]);
  }

  // Valid UTF-8 or not, for a few byte sequences.
  func utf8(k: u32) -> bool {
    local cases: list<list<u8>> = [
      [104, 105],
      [195, 169],
      [192, 128],
      [237, 160, 128],
      [244, 144, 128, 128],
      [226, 130],
      [240, 157, 132, 158],
      [128],
      [224, 160, 128],
      [239, 191, 191],
      [225, 128, 65],
      [224, 159, 191],
      [244, 143, 191, 191],
      [245, 128, 128, 128],
      [],
    ];
    match (string::from_bytes(cases[k])) {
      case some(s) {
        return string::len(s) == list::len(cases[k]);
      }
      case none {
        return false;
      }
    }
  }

  // `string::from_bytes` copies its argument.
  func from_bytes_copies() -> u32 {
    local b: list<mut u8> = list::filled(3, 97);
    match (string::from_bytes(b)) {
      case some(s) {
        b[0] = 98;
        return u32(string::chars(s)[0]);
      }
      case none {
        return 0;
      }
    }
  }

  func char_ok(n: u32) -> bool {
    match (char::from_u32(n)) {
      case some(c) {
        return u32(c) == n;
      }
      case none {
        return false;
      }
    }
  }

  func char_lt(a: char, b: char) -> bool {
    return a < b;
  }

  func long_len() -> u32 {
    local s: string = "LONG";
    return string::len(s) + string::len("LONG") + string::len(long_global);
  }

  func long_eq() -> bool {
    return "LONG" == long_global;
  }

  // ---- Records, tuples ----

  // Two locals that hold the same record see the same object (§ v2.5.1).
  func record_sharing() -> s32 {
    local p: geo::point = geo::point { x: 3, y: 4 };
    local q: geo::point = p;
    q.y = 10;
    return p.x * 100 + p.y;
  }

  func bump_origin() -> s32 {
    origin.y = origin.y + 1;
    return origin.y;
  }

  func tuple_sum() -> u32 {
    local t: tuple<u32, string, bool> = tuple(5, "abc", true);
    if (t.2) {
      return t.0 + string::len(t.1);
    }
    return 0;
  }

  // ---- Enums, flags ----

  func color_index(c: color) -> u32 {
    match (c) {
      case red {
        return 0;
      }
      case green {
        return 1;
      }
      else {
        return 9;
      }
    }
  }

  func same_color(a: color, b: color) -> bool {
    return a == b;
  }

  func perms_of(n: u32) -> perms {
    local p: perms;
    if (n > 0) {
      p = p | perms { read };
    }
    if (n > 1) {
      p = p | perms { write, exec };
    }
    return p;
  }

  func can_write(p: perms) -> bool {
    return p.write;
  }

  // break and continue inside the arms of a match, in a loop.
  func count_colors() -> u32 {
    local cs: list<color> = [color::red, color::green, color::blue, color::green];
    local i: u32;
    local count: u32;
    while (i < list::len(cs)) {
      local c: color = cs[i];
      i = i + 1;
      match (c) {
        case green {
          count = count + 1;
          continue;
        }
        case blue {
          break;
        }
        else {
          count = count + 10;
        }
      }
      count = count + 100;
    }
    return count;
  }

  // ---- Variants, recursive types ----

  func total_area() -> u32 {
    local sum: u32;
    local i: u32;
    while (i < list::len(shapes)) {
      sum = sum + geo::area(shapes[i]);
      i = i + 1;
    }
    return sum;
  }

  func eval_expr() -> s32 {
    // -(1 + (2 + -10)) = 7
    return geo::eval(geo::expr::neg(geo::expr::add(tuple(
      geo::expr::lit(1),
      geo::expr::add(tuple(geo::expr::lit(2), geo::expr::neg(geo::expr::lit(10)))),
    ))));
  }

  func chain_sum(n: u32) -> u32 {
    return geo::sum_chain(geo::chain(n));
  }

  func tree_total() -> u32 {
    return geo::tree_sum(geo::tree::branch([
      geo::tree::leaf(1),
      geo::tree::branch([geo::tree::leaf(2), geo::tree::leaf(3)]),
      geo::tree::branch([]),
    ]));
  }

  // ---- Lists ----

  func list_ops() -> u32 {
    local xs: list<mut u16> = list::filled(4, 7);
    xs[1] = 65535;
    local frozen: list<u16> = list::freeze(xs);
    xs[2] = 1;
    local copy: list<mut u16> = list::copy(frozen);
    copy[0] = 0;
    return u32(frozen[0]) + u32(frozen[1]) + u32(frozen[2]) + u32(xs[2]) + u32(copy[0])
      + list::len(copy);
  }

  func signed_elems() -> s32 {
    local xs: list<s8> = [-1, -128, 127];
    return s32(xs[0]) + s32(xs[1]) + s32(xs[2]);
  }

  func bools() -> bool {
    local bs: list<mut bool> = list::filled(2, false);
    bs[1] = true;
    return bs[1] && !bs[0];
  }

  func copy_strings(k: u32) -> bool {
    local e: list<string> = ["a", "b"];
    if (k == 1) {
      e = [];
    }
    local c: list<mut string> = list::copy(e);
    if (k == 1) {
      return list::len(c) == 0;
    }
    c[0] = "z";
    return e[0] == "a" && c[0] == "z" && c[1] == "b";
  }

  // `list::filled` shares its element (§ v2.5.1).
  func filled_shares() -> s32 {
    local ps: list<geo::point> = list::filled(3, geo::point { x: 0, y: 0 });
    ps[0].y = 5;
    return ps[2].y;
  }

  func index(i: u32) -> u32 {
    local xs: list<u32> = [1, 2, 3];
    return xs[i];
  }

  // ---- Options, results ----

  func opt(n: u32) -> u32 {
    local o: option<u32> = option::none;
    if (n > 0) {
      o = option::some(n);
    }
    match (o) {
      case some(v) {
        return v * 2;
      }
      case none {
        return 99;
      }
    }
  }

  func nested_opt(k: u32) -> u32 {
    local o: option<option<u32>> = option::some(option::none);
    if (k == 1) {
      o = option::none;
    }
    if (k == 2) {
      o = option::some(option::some(5));
    }
    match (o) {
      case none {
        return 0;
      }
      case some(inner) {
        match (inner) {
          case none {
            return 1;
          }
          case some(v) {
            return v;
          }
        }
      }
    }
  }

  func global_opts() -> u32 {
    match (some_u32) {
      case some(v) {
        match (none_u32) {
          case none {
            return v;
          }
          case some(w) {
            return w;
          }
        }
      }
      case none {
        return 0;
      }
    }
  }

  func opt_string(k: u32) -> u32 {
    local o: option<string> = option::some("four");
    if (k == 1) {
      o = option::none;
    }
    match (o) {
      case some(s) {
        return string::len(s);
      }
      case none {
        return 0;
      }
    }
  }

  func res(n: u32) -> u32 {
    local r: result<u32, string> = result::err("bad");
    if (n > 0) {
      r = result::ok(n);
    }
    match (r) {
      case ok(v) {
        return v;
      }
      case err(e) {
        return string::len(e) + 100;
      }
    }
  }

  func res_unit(n: u32) -> bool {
    local r: result = result::err;
    if (n > 0) {
      r = result::ok;
    }
    match (r) {
      case ok {
        return true;
      }
      case err {
        return false;
      }
    }
  }
}
"#;

/// Length of the long literal: above the 1,000 bytes of `array.new_fixed` (§ v2.6.4).
const LONG: usize = 1500;

fn engine_and_component() -> &'static (Engine, Component) {
    static CELL: OnceLock<(Engine, Component)> = OnceLock::new();
    CELL.get_or_init(|| {
        let package = Package::new("test", "heap", "0.1.0").unwrap();
        let mut sources = SourceMap::default();
        let program = PROGRAM.replace("LONG", &"x".repeat(LONG));
        sources.add("src/h.wasl".into(), program);
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

    fn try_call(&mut self, name: &str, args: &[Val]) -> wasmtime::Result<Val> {
        let iface = self
            .instance
            .get_export_index(&mut self.store, None, INTERFACE)
            .expect("exported interface");
        let index = self
            .instance
            .get_export_index(&mut self.store, Some(&iface), name)
            .unwrap_or_else(|| panic!("exported function `{name}`"));
        let func = self.instance.get_func(&mut self.store, index).unwrap();
        let mut results = vec![Val::Bool(false)];
        func.call(&mut self.store, args, &mut results)?;
        Ok(results.pop().unwrap())
    }

    fn call(&mut self, name: &str, args: &[Val]) -> Val {
        self.try_call(name, args)
            .unwrap_or_else(|e| panic!("{name}{args:?}: {e:?}"))
    }
}

/// Calls a function on a fresh instance.
fn call(name: &str, args: &[Val]) -> Val {
    Runner::new().call(name, args)
}

#[test]
fn strings() {
    use Val::*;
    assert_eq!(call("greeting-len", &[]), U32(6));
    assert_eq!(call("str-eq", &[U32(0)]), Bool(true));
    assert_eq!(call("str-eq", &[U32(1)]), Bool(false));
    assert_eq!(call("str-eq", &[U32(2)]), Bool(true));
    assert_eq!(call("str-eq", &[U32(3)]), Bool(true));
    assert_eq!(call("concat-len", &[]), U32(13));
    assert_eq!(call("nth-char", &[U32(0)]), Char('h'));
    assert_eq!(call("nth-char", &[U32(1)]), Char('é'));
    assert_eq!(call("nth-char", &[U32(4)]), Char('o'));
    assert_eq!(call("char-count", &[]), U32(4));
    assert_eq!(call("roundtrip", &[]), Bool(true));
    assert_eq!(call("bytes-sum", &[]), U32(131));
    assert_eq!(call("char-lt", &[Char('a'), Char('é')]), Bool(true));
    assert_eq!(call("char-lt", &[Char('𝄞'), Char('z')]), Bool(false));
    let long = LONG as u32;
    assert_eq!(call("long-len", &[]), U32(3 * long));
    assert_eq!(call("long-eq", &[]), Bool(true));
}

#[test]
fn utf8() {
    use Val::*;
    let expected = [
        true, true, false, false, false, false, true, false, true, true, false, false, true, false,
        true,
    ];
    for (k, valid) in expected.into_iter().enumerate() {
        assert_eq!(call("utf8", &[U32(k as u32)]), Bool(valid), "case {k}");
    }
    assert_eq!(call("from-bytes-copies", &[]), U32(97));
    for (n, valid) in [
        (0x41, true),
        (0xD7FF, true),
        (0xD800, false),
        (0xDFFF, false),
        (0xE000, true),
        (0x10FFFF, true),
        (0x110000, false),
        (u32::MAX, false),
    ] {
        assert_eq!(call("char-ok", &[U32(n)]), Bool(valid), "{n:#x}");
    }
}

#[test]
fn records_and_tuples() {
    use Val::*;
    assert_eq!(call("record-sharing", &[]), S32(310));
    let mut runner = Runner::new();
    assert_eq!(runner.call("bump-origin", &[]), S32(3));
    assert_eq!(runner.call("bump-origin", &[]), S32(4));
    assert_eq!(call("tuple-sum", &[]), U32(8));
}

#[test]
fn enums_and_flags() {
    use Val::*;
    let color = |c: &str| Enum(c.into());
    assert_eq!(call("color-index", &[color("red")]), U32(0));
    assert_eq!(call("color-index", &[color("green")]), U32(1));
    assert_eq!(call("color-index", &[color("blue")]), U32(9));
    assert_eq!(
        call("same-color", &[color("blue"), color("blue")]),
        Bool(true)
    );
    assert_eq!(
        call("same-color", &[color("red"), color("blue")]),
        Bool(false)
    );
    assert_eq!(call("perms-of", &[U32(0)]), Flags(vec![]));
    assert_eq!(call("perms-of", &[U32(1)]), Flags(vec!["read".into()]));
    assert_eq!(
        call("perms-of", &[U32(2)]),
        Flags(vec!["read".into(), "write".into(), "exec".into()])
    );
    assert_eq!(
        call("can-write", &[Flags(vec!["write".into()])]),
        Bool(true)
    );
    assert_eq!(
        call("can-write", &[Flags(vec!["read".into(), "exec".into()])]),
        Bool(false)
    );
    assert_eq!(call("count-colors", &[]), U32(111));
}

#[test]
fn variants() {
    use Val::*;
    assert_eq!(call("total-area", &[]), U32(24));
    assert_eq!(call("chain-sum", &[U32(1)]), U32(0));
    assert_eq!(call("chain-sum", &[U32(100)]), U32(4950));
    assert_eq!(call("tree-total", &[]), U32(6));
    assert_eq!(call("eval-expr", &[]), S32(7));
}

#[test]
fn lists() {
    use Val::*;
    assert_eq!(call("list-ops", &[]), U32(65554));
    assert_eq!(call("signed-elems", &[]), S32(-2));
    assert_eq!(call("bools", &[]), Bool(true));
    assert_eq!(call("copy-strings", &[U32(0)]), Bool(true));
    assert_eq!(call("copy-strings", &[U32(1)]), Bool(true));
    assert_eq!(call("filled-shares", &[]), S32(5));
    assert_eq!(call("index", &[U32(2)]), U32(3));
    let err = Runner::new().try_call("index", &[U32(3)]).unwrap_err();
    assert!(
        format!("{err:?}").contains("out of bounds array access"),
        "{err:?}"
    );
}

#[test]
fn options_and_results() {
    use Val::*;
    assert_eq!(call("opt", &[U32(0)]), U32(99));
    assert_eq!(call("opt", &[U32(4)]), U32(8));
    assert_eq!(call("nested-opt", &[U32(0)]), U32(1));
    assert_eq!(call("nested-opt", &[U32(1)]), U32(0));
    assert_eq!(call("nested-opt", &[U32(2)]), U32(5));
    assert_eq!(call("global-opts", &[]), U32(7));
    assert_eq!(call("opt-string", &[U32(0)]), U32(4));
    assert_eq!(call("opt-string", &[U32(1)]), U32(0));
    assert_eq!(call("res", &[U32(0)]), U32(103));
    assert_eq!(call("res", &[U32(5)]), U32(5));
    assert_eq!(call("res-unit", &[U32(0)]), Bool(false));
    assert_eq!(call("res-unit", &[U32(1)]), Bool(true));
}

/// The functions of the target program (§ v2.10.1), called from an interface whose
/// functions only take and return scalars.
const INVOICE_PROBE: &str = r#"
world probe {
  export p;
}

interface p {
  func total_sample(discount: u8) -> u64 {
    local inv: invoices::invoice = invoices::sample();
    inv.status = invoices::status::sent;
    local d: invoices::invoice = invoices::invoice {
      number: inv.number,
      customer: inv.customer,
      lines: inv.lines,
      discount: option::some(invoices::discount::percent(discount)),
      delivery: inv.delivery,
      status: inv.status,
    };
    match (invoices::total(d)) {
      case ok(t) {
        return t;
      }
      case err(e) {
        return 1000000 + u64(q::error_code(e));
      }
    }
  }

  func summary_matches() -> bool {
    return invoices::summary(invoices::sample())
      == "Weasel & Co: invoice 42 for ACME (draft)";
  }

  func initial() -> char {
    match (invoices::initial(invoices::sample())) {
      case some(c) {
        return c;
      }
      case none {
        return '?';
      }
    }
  }

  func stats_sum() -> u64 {
    local inv: invoices::invoice = invoices::sample();
    local all: list<invoices::invoice> = [inv, inv, inv];
    return invoices::stats(all).1;
  }

  func unpaid_after_mark() -> u32 {
    local inv: invoices::invoice = invoices::sample();
    local other: invoices::invoice = invoices::sample();
    match (invoices::mark_paid(inv)) {
      case ok(paid) {
        return invoices::count_unpaid([inv, other, paid]);
      }
      case err {
        return 99;
      }
    }
  }

  func number_text(n: u32) -> u32 {
    return string::len(text::from_u32(n));
  }
}

// Internal: its function takes a type of an interface that `probe` does not export.
interface q {
  func error_code(e: invoices::invoice_error) -> u32 {
    match (e) {
      case empty_customer {
        return 1;
      }
      case no_lines {
        return 2;
      }
      case zero_quantity {
        return 3;
      }
      case discount_too_large {
        return 4;
      }
    }
  }
}
"#;

#[test]
fn invoice_functions() {
    use Val::*;
    let package = Package::new("test", "probe", "0.1.0").unwrap();
    let mut sources = SourceMap::default();
    sources.add(
        "src/invoice.wasl".into(),
        include_str!("../../examples/invoice/src/invoice.wasl").to_string(),
    );
    sources.add("src/probe.wasl".into(), INVOICE_PROBE.to_string());
    let bytes = waslc::analyze(&package, &sources)
        .and_then(|analysis| analysis.build("probe"))
        .unwrap_or_else(|e| panic!("{}", e.display(&sources)));
    let engine = Engine::default();
    let component = Component::new(&engine, &bytes).unwrap();
    let mut store = Store::new(&engine, ());
    let instance = Linker::new(&engine)
        .instantiate(&mut store, &component)
        .unwrap();
    let mut call = |name: &str, args: &[Val]| {
        let iface = instance
            .get_export_index(&mut store, None, "test:probe/p@0.1.0")
            .unwrap();
        let index = instance
            .get_export_index(&mut store, Some(&iface), name)
            .unwrap();
        let func = instance.get_func(&mut store, index).unwrap();
        let mut results = vec![Bool(false)];
        func.call(&mut store, args, &mut results).unwrap();
        results.pop().unwrap()
    };
    // 3 × 150 + 2 × 425 = 1300 cents
    assert_eq!(call("total-sample", &[U8(10)]), U64(1170));
    assert_eq!(call("total-sample", &[U8(0)]), U64(1300));
    assert_eq!(call("total-sample", &[U8(101)]), U64(1000004));
    assert_eq!(call("summary-matches", &[]), Bool(true));
    assert_eq!(call("initial", &[]), Char('A'));
    assert_eq!(call("stats-sum", &[]), U64(3 * 1170));
    // `mark_paid` changes the record it receives: inside the component, `inv` and `paid`
    // are the same record (§ v2.5.4).
    assert_eq!(call("unpaid-after-mark", &[]), U32(1));
    assert_eq!(call("number-text", &[U32(0)]), U32(1));
    assert_eq!(call("number-text", &[U32(4294967295)]), U32(10));
}
