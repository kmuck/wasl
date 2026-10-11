//! Values that cross the component boundary (specification v0.2, chapter v2.7): the
//! adapters lift the arguments from linear memory and lower the results into it.

use wasmtime::component::{Component, Instance, Linker, Val};
use wasmtime::{Engine, Store};

use waslc::{Package, SourceMap};

/// A component built from one source file, and an instance of it.
struct Runner {
    store: Store<()>,
    instance: Instance,
}

impl Runner {
    fn new(package: &str, source: &str, world: &str) -> Runner {
        let package = Package::new("test", package, "0.1.0").unwrap();
        let mut sources = SourceMap::default();
        sources.add("src/main.wasl".into(), source.to_string());
        let bytes = waslc::analyze(&package, &sources)
            .and_then(|analysis| analysis.build(world))
            .unwrap_or_else(|e| panic!("{}", e.display(&sources)));
        let engine = Engine::default();
        let component = Component::new(&engine, &bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Linker::new(&engine)
            .instantiate(&mut store, &component)
            .unwrap();
        Runner { store, instance }
    }

    fn call(&mut self, iface: &str, name: &str, args: &[Val]) -> Option<Val> {
        let iface = self
            .instance
            .get_export_index(&mut self.store, None, iface)
            .unwrap_or_else(|| panic!("interface `{iface}`"));
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

fn record(fields: &[(&str, Val)]) -> Val {
    Val::Record(
        fields
            .iter()
            .map(|(n, v)| (n.to_string(), v.clone()))
            .collect(),
    )
}

fn string(s: &str) -> Val {
    Val::String(s.into())
}

fn some(v: Val) -> Val {
    Val::Option(Some(Box::new(v)))
}

fn ok(v: Val) -> Val {
    Val::Result(Ok(Some(Box::new(v))))
}

fn err(v: Val) -> Val {
    Val::Result(Err(Some(Box::new(v))))
}

fn line(label: &str, quantity: u32, unit_price: u64) -> Val {
    record(&[
        ("label", string(label)),
        ("quantity", Val::U32(quantity)),
        ("unit-price", Val::U64(unit_price)),
    ])
}

fn invoice(number: u32, customer: &str, lines: Vec<Val>, status: &str) -> Val {
    record(&[
        ("number", Val::U32(number)),
        ("customer", string(customer)),
        ("lines", Val::List(lines)),
        (
            "discount",
            some(Val::Variant("percent".into(), Some(Box::new(Val::U8(10))))),
        ),
        (
            "delivery",
            Val::Flags(vec!["email".into(), "urgent".into()]),
        ),
        ("status", Val::Enum(status.into())),
    ])
}

fn sample() -> Val {
    invoice(
        42,
        "ACME",
        vec![line("pen", 3, 150), line("notebook", 2, 425)],
        "draft",
    )
}

/// The target program of v0.2 (§ v2.10.1), called from the host.
#[test]
fn invoice_example() {
    use Val::*;
    let mut r = Runner::new(
        "invoice",
        include_str!("../../examples/invoice/src/invoice.wasl"),
        "billing",
    );
    let iface = "test:invoice/invoices@0.1.0";
    let mut call = |name: &str, args: &[Val]| r.call(iface, name, args).unwrap();

    assert_eq!(call("sample", &[]), sample());
    assert_eq!(call("total", &[sample()]), ok(U64(1170)));
    let empty = invoice(1, "", vec![line("x", 1, 1)], "sent");
    assert_eq!(call("total", &[empty]), err(Enum("empty-customer".into())));
    let zero = invoice(1, "B", vec![line("x", 0, 1)], "sent");
    assert_eq!(call("total", &[zero]), err(Enum("zero-quantity".into())));

    let paid = invoice(
        42,
        "ACME",
        vec![line("pen", 3, 150), line("notebook", 2, 425)],
        "paid",
    );
    assert_eq!(call("mark-paid", &[sample()]), ok(paid.clone()));
    assert_eq!(
        call("summary", &[sample()]),
        string("Weasel & Co: invoice 42 for ACME (draft)")
    );
    assert_eq!(call("needs-printing", &[sample()]), Bool(false));
    assert_eq!(call("initial", &[sample()]), some(Char('A')));
    assert_eq!(
        call("initial", &[invoice(1, "", vec![], "draft")]),
        Option(None)
    );

    let other = invoice(7, "Globex", vec![line("x", 1, 1000)], "sent");
    let all = List(vec![sample(), other.clone(), paid]);
    assert_eq!(
        call("stats", std::slice::from_ref(&all)),
        Tuple(vec![U32(3), U64(1170 + 900 + 1170)])
    );
    assert_eq!(call("find", &[all.clone(), string("Globex")]), some(other));
    assert_eq!(call("find", &[all.clone(), string("Nobody")]), Option(None));
    assert_eq!(call("count-unpaid", &[all]), U32(2));
    assert_eq!(call("count-unpaid", &[List(vec![])]), U32(0));
}

const TYPES: &str = r#"
world shop {
  export orders;
}

world both {
  export types;
  export orders;
}

interface types {
  record item {
    name: string,
    price: u64,
  }
}

interface orders {
  func cheapest(items: list<types::item>) -> option<types::item> {
    if (list::len(items) == 0) {
      return option::none;
    }
    local best: types::item = items[0];
    local i: u32 = 1;
    while (i < list::len(items)) {
      if (items[i].price < best.price) {
        best = items[i];
      }
      i = i + 1;
    }
    return option::some(best);
  }
}
"#;

/// A types-only interface (§ v2.7.1, open question 6): imported implicitly by a world
/// that uses its types, or exported along with the interface that uses them.
#[test]
fn types_only_interface() {
    let items = Val::List(vec![
        record(&[("name", string("tea")), ("price", Val::U64(300))]),
        record(&[("name", string("cake")), ("price", Val::U64(250))]),
    ]);
    let expected = some(record(&[
        ("name", string("cake")),
        ("price", Val::U64(250)),
    ]));
    for world in ["shop", "both"] {
        let mut r = Runner::new("shop", TYPES, world);
        assert_eq!(
            r.call(
                "test:shop/orders@0.1.0",
                "cheapest",
                std::slice::from_ref(&items)
            ),
            Some(expected.clone()),
            "world {world}"
        );
    }
}

const VALUES: &str = r#"
world w {
  export v;
}

interface v {
  enum level {
    low,
    high,
  }

  flags opts {
    a,
    b,
    c,
    d,
    e,
    f,
    g,
    h,
    i,
  }

  variant mixed {
    small(f32),
    big(u64),
    text(string),
    nothing,
  }

  record one {
    value: u32,
  }

  record wide {
    a: u8,
    b: u64,
    c: s16,
    d: bool,
    e: f64,
    f: char,
    g: level,
    h: opts,
    i: tuple<u8, string>,
    j: option<u16>,
    k: list<s8>,
  }

  variant tag {
    first,
    second,
  }

  func echo(s: string) -> string {
    return s + "!";
  }

  func mixed_round(m: mixed) -> mixed {
    return m;
  }

  func describe(m: mixed) -> string {
    match (m) {
      case small(x) {
        if (x > 1.0) {
          return "small>1";
        }
        return "small";
      }
      case big(n) {
        if (n > 4294967296) {
          return "big>2^32";
        }
        return "big";
      }
      case text(t) {
        return "text:" + t;
      }
      case nothing {
        return "nothing";
      }
    }
  }

  func wide_round(w: wide) -> wide {
    return w;
  }

  func make_one(n: u32) -> one {
    return one { value: n + 1 };
  }

  func tag_of(n: u32) -> tag {
    if (n == 0) {
      return tag::first;
    }
    return tag::second;
  }

  func pair(a: u32) -> tuple<u32, u32> {
    return tuple(a, a * 2);
  }

  func seventeen(a: u32, b: u32, c: u32, d: u32, e: u32, f: u32, g: u32, h: u32, i: u32,
                 j: u32, k: u32, l: u32, m: u32, n: u32, o: u32, p: u32, q: string) -> u32 {
    return a + b + c + d + e + f + g + h + i + j + k + l + m + n + o + p + string::len(q);
  }

  func nested(xs: list<list<string>>) -> list<list<string>> {
    return xs;
  }

  func lists(b: list<bool>, s: list<s16>, f: list<f64>) -> tuple<list<bool>, list<s16>, list<f64>> {
    return tuple(b, s, f);
  }

  func options(o: option<option<u32>>, r: result<_, string>) -> tuple<option<option<u32>>, result<_, string>> {
    return tuple(o, r);
  }

  func count_bytes(xs: list<mut u8>) -> u32 {
    xs[0] = 0;
    return list::len(xs);
  }
}
"#;

#[test]
fn values() {
    use Val::*;
    let mut r = Runner::new("values", VALUES, "w");
    let iface = "test:values/v@0.1.0";
    let mut call = |name: &str, args: &[Val]| r.call(iface, name, args).unwrap();

    assert_eq!(call("echo", &[string("héllo")]), string("héllo!"));
    // Memory grows to hold large values, and is reused between calls.
    let big = "x".repeat(200_000);
    assert_eq!(call("echo", &[string(&big)]), string(&(big + "!")));
    assert_eq!(call("echo", &[string("")]), string("!"));

    let mixed = [
        Variant("small".into(), Some(Box::new(Float32(2.5)))),
        Variant("big".into(), Some(Box::new(U64(u64::MAX)))),
        Variant("text".into(), Some(Box::new(string("abc")))),
        Variant("nothing".into(), None),
    ];
    for m in &mixed {
        assert_eq!(&call("mixed-round", std::slice::from_ref(m)), m);
    }
    assert_eq!(call("describe", &[mixed[0].clone()]), string("small>1"));
    assert_eq!(call("describe", &[mixed[1].clone()]), string("big>2^32"));
    assert_eq!(call("describe", &[mixed[2].clone()]), string("text:abc"));
    assert_eq!(call("describe", &[mixed[3].clone()]), string("nothing"));

    let wide = record(&[
        ("a", U8(255)),
        ("b", U64(1 << 40)),
        ("c", S16(-300)),
        ("d", Bool(true)),
        ("e", Float64(-0.5)),
        ("f", Char('𝄞')),
        ("g", Enum("high".into())),
        ("h", Flags(vec!["a".into(), "i".into()])),
        ("i", Tuple(vec![U8(7), string("seven")])),
        ("j", some(U16(65535))),
        ("k", List(vec![S8(-128), S8(0), S8(127)])),
    ]);
    assert_eq!(call("wide-round", std::slice::from_ref(&wide)), wide);

    assert_eq!(call("make-one", &[U32(41)]), record(&[("value", U32(42))]));
    assert_eq!(
        call("tag-of", &[U32(0)]),
        Enum("first".into()).into_variant()
    );
    assert_eq!(
        call("tag-of", &[U32(1)]),
        Enum("second".into()).into_variant()
    );
    assert_eq!(call("pair", &[U32(21)]), Tuple(vec![U32(21), U32(42)]));

    let mut args: Vec<Val> = (1..=16).map(U32).collect();
    args.push(string("abc"));
    assert_eq!(call("seventeen", &args), U32(136 + 3));

    let nested = List(vec![
        List(vec![string("a"), string("bc")]),
        List(vec![]),
        List(vec![string("")]),
    ]);
    assert_eq!(call("nested", std::slice::from_ref(&nested)), nested);

    let lists = [
        List(vec![Bool(true), Bool(false), Bool(true)]),
        List(vec![S16(-1), S16(i16::MAX), S16(i16::MIN)]),
        List(vec![Float64(1.5), Float64(f64::MAX)]),
    ];
    assert_eq!(call("lists", &lists), Tuple(lists.to_vec()));

    for (o, res) in [
        (Option(None), Result(Ok(None))),
        (some(Option(None)), err(string("e"))),
        (some(some(U32(9))), Result(Ok(None))),
    ] {
        assert_eq!(
            call("options", &[o.clone(), res.clone()]),
            Tuple(vec![o, res])
        );
    }

    assert_eq!(call("count-bytes", &[List(vec![U8(1), U8(2)])]), U32(2));
}

trait IntoVariant {
    fn into_variant(self) -> Val;
}

impl IntoVariant for Val {
    /// A case without payload of a variant.
    fn into_variant(self) -> Val {
        match self {
            Val::Enum(name) => Val::Variant(name, None),
            other => other,
        }
    }
}
