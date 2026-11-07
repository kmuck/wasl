//! Compilation of valid and invalid programs (specification, chapter 15).

use waslc::diag::Code;
use waslc::source::SourceMap;
use waslc::{Analysis, Dependencies, Diagnostic, Package};

/// Analyzes a package, and returns its first error.
fn analyze(
    package: &Package,
    sources: &SourceMap,
    deps: &Dependencies,
) -> Result<Analysis, Diagnostic> {
    waslc::analyze(package, sources, deps).map_err(|mut e| e.remove(0))
}

fn package() -> Package {
    Package::new("example", "calc", "0.1.0").unwrap()
}

fn sources(files: &[(&str, &str)]) -> SourceMap {
    let mut sources = SourceMap::default();
    for (path, text) in files {
        sources.add(path.into(), text.to_string());
    }
    sources
}

/// Analyzes the given files; errors are rendered as the user sees them.
fn analyze_files(files: &[(&str, &str)]) -> Result<Analysis, String> {
    let sources = sources(files);
    analyze(&package(), &sources, &Default::default()).map_err(|e| e.display(&sources).to_string())
}

fn validate(bytes: &[u8]) {
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(bytes)
        .expect("valid component");
}

const CALC: &str = include_str!("fixtures/calc.wasl");

#[test]
fn spec_example() {
    let analysis = analyze_files(&[("src/calc.wasl", CALC)]).unwrap();
    let worlds: Vec<_> = analysis.worlds().iter().map(|w| w.name.as_str()).collect();
    assert_eq!(worlds, ["calculator"]);
    validate(&analysis.build("calculator").unwrap());

    let wit = analysis.wit().unwrap();
    for expected in [
        "package example:calc@0.1.0;",
        "interface math {",
        "fib-rec: func(n: u32) -> u64;",
        "circle-area: func(r: f64) -> f64;",
        "clamp: func(x: s32, lo: s32, hi: s32) -> s32;",
        "reset: func();",
        "world calculator {",
        "export math;",
    ] {
        assert!(wit.contains(expected), "{expected}\n{wit}");
    }
    // The internal interface does not appear in the WIT.
    assert!(!wit.contains("util"), "{wit}");
}

#[test]
fn multiple_files_and_worlds() {
    let analysis = analyze_files(&[
        (
            "src/a/math.wasl",
            "interface math { func one() -> u32 { return helpers::one(); } }",
        ),
        (
            "src/b.wasl",
            "global unused: u32 = 1; interface helpers { func one() -> u32 { return 1; } }",
        ),
        (
            "src/worlds.wasl",
            "world first { export math; } world second { export math; export helpers; } world empty { }",
        ),
    ])
    .unwrap();
    let worlds: Vec<_> = analysis.worlds().iter().map(|w| w.name.as_str()).collect();
    assert_eq!(worlds, ["first", "second", "empty"]);
    for world in &worlds {
        validate(&analysis.build(world).unwrap());
    }
    let exports: Vec<_> = analysis.worlds()[1]
        .exports
        .iter()
        .map(|e| (e.name.as_str(), e.functions.clone()))
        .collect();
    assert_eq!(
        exports,
        [
            ("math", vec!["one".to_string()]),
            ("helpers", vec!["one".to_string()])
        ]
    );
    assert!(analysis.worlds()[2].exports.is_empty());

    let err = analysis.build("third").unwrap_err();
    assert_eq!(err.code, Code::UnknownWorld);
    assert_eq!(
        err.message,
        "unknown world `third`: the package defines `first`, `second`, `empty`"
    );
}

#[test]
fn error_format() {
    let err = analyze_files(&[(
        "src/math.wasl",
        "interface math {\n  func f(n: u32) -> u64 {\n    return n;\n  }\n}\n",
    )])
    .err()
    .unwrap();
    assert_eq!(
        err,
        "src/math.wasl:3:12: error[E0400]: mismatched types: expected `u64`, found `u32`"
    );
}

#[test]
fn first_error_follows_file_order() {
    let err = analyze_files(&[
        (
            "src/a.wasl",
            "interface a { func f() -> u32 { return x; } }",
        ),
        (
            "src/b.wasl",
            "interface b { func g() -> u32 { return y; } }",
        ),
    ])
    .err()
    .unwrap();
    assert!(err.starts_with("src/a.wasl:"), "{err}");
}

/// Invalid programs from the specification (§ 15.4) and their expected error code.
#[test]
fn invalid_programs() {
    let cases: &[(&str, Code)] = &[
        ("local x = 5;", Code::Syntax),
        ("local x: u8 = 256;", Code::LiteralOutOfRange),
        ("local x: f64 = 1;", Code::TypeMismatch),
        ("local y: u64 = n;", Code::TypeMismatch),
        ("if (n) { }", Code::NonBoolCondition),
        ("if (n > 0) return;", Code::Syntax),
        ("if n > 0 { }", Code::Syntax),
        ("pi = 3.0;", Code::AssignImmutable),
        ("local pi: f64 = 1.0;", Code::ShadowsGlobal),
        ("local z: s32 = -n;", Code::TypeMismatch),
        ("local r: f64 = x % x;", Code::InvalidOperator),
        ("if (n < n < n) { }", Code::Syntax),
        ("n + 1;", Code::Syntax),
        ("local w: u8 = u8(5);", Code::UntypedLiteral),
        ("local q: u32 = 007;", Code::InvalidIntLiteral),
        ("local q: f64 = 1.;", Code::InvalidFloatLiteral),
        ("local Q: u32 = 1;", Code::InvalidIdent),
        ("local record: u32 = 1;", Code::ReservedKeyword),
        ("local match: u32 = 1;", Code::ReservedKeyword),
        ("local import: u32 = 1;", Code::ReservedKeyword),
        ("local while: u32 = 1;", Code::ReservedKeyword),
        ("local string: u32 = 1;", Code::ReservedKeyword),
        ("local q: u32 = 1 @ 2;", Code::InvalidChar),
        ("local s: string = \"abc;", Code::InvalidString),
        ("local s: string = \"\\q\";", Code::InvalidString),
        ("local c: char = '';", Code::InvalidCharLiteral),
        ("local c: u32 = u32(char(n));", Code::InvalidConversion),
        ("continue;", Code::BreakOutsideLoop),
        ("return 1;", Code::UnexpectedReturnValue),
        ("unknown();", Code::UnknownFunc),
        ("other::g(1);", Code::UnknownName),
        ("t::f(1);", Code::ArgCount),
    ];
    for (body, code) in cases {
        let program = format!(
            "global pi: f64 = 3.14;
             interface t {{ func f(n: u32, x: f64) {{ {body} }} }}"
        );
        let sources = sources(&[("src/t.wasl", &program)]);
        match analyze(&package(), &sources, &Default::default()) {
            Ok(_) => panic!("accepted: {body}"),
            Err(e) => assert_eq!(e.code, *code, "{body}: {}", e.display(&sources)),
        }
    }

    let missing_return = analyze_files(&[(
        "src/t.wasl",
        "interface t { func f(x: u32) -> u32 { if (x > 0) { return x; } } }",
    )])
    .err()
    .unwrap();
    assert!(missing_return.contains("E0504"), "{missing_return}");

    let global_init = analyze_files(&[("src/t.wasl", "global g: u32 = 1 + 1;")])
        .err()
        .unwrap();
    assert!(global_init.contains("E0409"), "{global_init}");
}

/// Errors of `?` (§ 8.19): E0414, E0415, and the value rules of § 9.6.
/// Destructuring (§ 9.4): its syntax, its types and its names.
#[test]
fn destructuring_errors() {
    let cases: &[(&str, Code, &str)] = &[
        (
            "local (a: u32, b: u64) = t::pair();",
            Code::TypeMismatch,
            "`b` is declared `u64`",
        ),
        (
            "local (a: u32) = t::pair();",
            Code::TypeMismatch,
            "has 2 element(s), and the destructuring has 1",
        ),
        (
            "local (a: u32, b: u32, _) = t::pair();",
            Code::TypeMismatch,
            "has 2 element(s)",
        ),
        (
            "local (a: u32, b: u32) = n;",
            Code::TypeMismatch,
            "takes the elements of a tuple",
        ),
        (
            "local (a: u32, b: u32) = tuple(1, 2, 3);",
            Code::TypeMismatch,
            "a tuple of 3 element(s)",
        ),
        (
            "local (a: u32, _) = tuple(1, 2);",
            Code::CannotInfer,
            "cannot infer",
        ),
        (
            "local (a: u32, a: u32) = t::pair();",
            Code::DuplicateLocal,
            "already declared",
        ),
        (
            "local (n: u32, b: u32) = t::pair();",
            Code::DuplicateLocal,
            "already declared",
        ),
        (
            "local (pi: u32, _) = t::pair();",
            Code::ShadowsGlobal,
            "name of a global",
        ),
        (
            "local (a: u32, b: u32) = tuple(a, 1);",
            Code::UnknownName,
            "`a`",
        ),
        ("local (a: u32, b: u32);", Code::Syntax, "`=`"),
        (
            "local ((a: u32, b: u32), c: u32) = t::pair();",
            Code::Syntax,
            "a local name or `_`",
        ),
        ("local (a, b) = t::pair();", Code::Syntax, "`:`"),
        ("local () = t::pair();", Code::Syntax, "a local name or `_`"),
    ];
    for (body, code, message) in cases {
        let program = format!(
            "global pi: f64 = 3.14;
             interface t {{
               func pair() -> tuple<u32, u32> {{ return tuple(1, 2); }}
               func f(n: u32) {{ {body} }}
             }}"
        );
        let sources = sources(&[("src/t.wasl", &program)]);
        match analyze(&package(), &sources, &Default::default()) {
            Ok(_) => panic!("accepted: {body}"),
            Err(e) => {
                assert_eq!(e.code, *code, "{body}: {}", e.display(&sources));
                assert!(e.message.contains(message), "{body}: {}", e.message);
            }
        }
    }
    // The locals are visible after the declaration, and a tuple of literals takes the
    // written types.
    analyze_files(&[(
        "src/t.wasl",
        "interface t {
           func f() -> u64 {
             local (a: u32, b: u64,) = tuple(1, 2);
             local (_, c: u64) = tuple(a, b + 1);
             return c;
           }
         }",
    )])
    .unwrap();
}

#[test]
fn error_propagation_errors() {
    let cases: &[(&str, &str, Code)] = &[
        (
            "-> option<u32>",
            "local y: u32 = n?; return option::none;",
            Code::TryOperand,
        ),
        ("", "_ = r()?;", Code::TryReturn),
        ("-> u32", "return r()?;", Code::TryReturn),
        (
            "-> option<u32>",
            "return option::some(r()?);",
            Code::TryReturn,
        ),
        (
            "-> result<u32, s8>",
            "return result::ok(r()?);",
            Code::TryReturn,
        ),
        (
            "-> result<u32>",
            "return result::ok(r()?);",
            Code::TryReturn,
        ),
        (
            "-> result<u32, e>",
            "return result::ok(o()?);",
            Code::TryReturn,
        ),
        (
            "-> result<u32, e>",
            "r()?; return result::ok(1);",
            Code::IgnoredValue,
        ),
        (
            "-> result<u32, e>",
            "_ = u()?; return result::ok(1);",
            Code::TypeMismatch,
        ),
        (
            "-> result<u32, e>",
            "local y: u32 = u()?; return result::ok(y);",
            Code::TypeMismatch,
        ),
        ("-> result<u32, e>", "return r()?;", Code::TypeMismatch),
        (
            "-> result<u32, e>",
            "local y: u32 = n ? 1 : 2;",
            Code::Syntax,
        ),
    ];
    for (ret, body, code) in cases {
        let program = format!(
            "interface t {{
               enum e {{ a, b }}
               func r() -> result<u32, e> {{ return result::ok(1); }}
               func u() -> result<_, e> {{ return result::ok; }}
               func o() -> option<u32> {{ return option::none; }}
               func f(n: u32) {ret} {{ {body} }}
             }}"
        );
        let sources = sources(&[("src/t.wasl", &program)]);
        match analyze(&package(), &sources, &Default::default()) {
            Ok(_) => panic!("accepted: {body}"),
            Err(e) => assert_eq!(e.code, *code, "{body}: {}", e.display(&sources)),
        }
    }

    let global = analyze_files(&[(
        "src/t.wasl",
        "global g: u32 = t::r()?;
         interface t { func r() -> result<u32> { return result::ok(1); } }",
    )])
    .err()
    .unwrap();
    assert!(global.contains("E0409"), "{global}");
}

/// Types in the WIT: declarations, `use` of types-only interfaces, erased `mut`
/// (§ 13.1).
#[test]
fn wit_of_types() {
    let analysis = analyze_files(&[(
        "src/shop.wasl",
        "world shop { export orders; }
         interface types {
           record item { name: string, price: u64 }
         }
         interface orders {
           enum status { draft, paid_late }
           flags delivery { email, paper }
           variant discount { percent(u8), fixed(u64), none }
           record order {
             items: list<types::item>,
             status: status,
             delivery: delivery,
             discount: option<discount>,
             totals: tuple<u32, u64>,
             check: result<_, status>,
             done: result,
             first: char,
           }
           record item { label: string }
           func cheapest(items: list<types::item>) -> u32 { return 0; }
           func mine(i: item, j: types::item) -> result<order, status> { return mine(i, j); }
         }
         interface internal { record hidden { a: u32 } }",
    )])
    .unwrap();
    let wit = analysis.wit().unwrap();
    for expected in [
        "interface types {\n  record item {\n    name: string,\n    price: u64,\n  }\n}",
        "use types.{item as types-item};",
        "enum status {\n    draft,\n    paid-late,\n  }",
        "flags delivery {\n    email,\n    paper,\n  }",
        "variant discount {\n    percent(u8),\n    fixed(u64),\n    none,\n  }",
        "items: list<types-item>,",
        "status: status,",
        "discount: option<discount>,",
        "totals: tuple<u32, u64>,",
        "check: result<_, status>,",
        "done: result,",
        "first: char,",
        "cheapest: func(items: list<types-item>) -> u32;",
        "mine: func(i: item, j: types-item) -> result<order, status>;",
        "world shop {\n  export orders;\n}",
    ] {
        assert!(wit.contains(expected), "{expected}\n{wit}");
    }
    assert!(!wit.contains("hidden"), "{wit}");
}

/// Types that appear in the WIT cannot change (§ 7.11): E0214.
#[test]
fn mutable_types_stay_inside() {
    let program = |exported: &str| {
        format!(
            "world w {{ export api; }}
             interface shared {{ record counter {{ n: mut u32 }} record point {{ x: u32 }} }}
             interface inner {{
               record cell {{ v: mut u32 }}
               func bump(c: cell, xs: list<mut u8>) -> list<mut u8> {{ c.v = c.v + 1; xs[0] = 1; return xs; }}
             }}
             interface api {{
               {exported}
             }}"
        )
    };
    // Mutable types are allowed inside the component, and so are types of `shared`
    // that no exported function uses.
    let ok = [
        "func f(p: shared::point) -> u32 { return p.x; }",
        "func f(xs: list<u8>) -> u32 { local ys: list<mut u8> = list::copy(xs); ys[0] = 2; return list::len(ys); }",
        "record r { a: list<list<u8>> } func f(x: r) -> r { return x; }",
    ];
    for exported in ok {
        if let Err(e) = analyze_files(&[("src/m.wasl", &program(exported))]) {
            panic!("{exported}: {e}");
        }
    }
    let cases = [
        (
            "record r { a: mut u32 }",
            "field `a` of record `api::r` is `mut`",
        ),
        (
            "func f(xs: list<mut u8>) { }",
            "`list<mut u8>` appears in the WIT through the exported function `f`",
        ),
        ("func f() -> list<mut u8> { return [1]; }", "`list<mut u8>`"),
        (
            "func f(c: shared::counter) { }",
            "field `n` of record `shared::counter`",
        ),
        (
            "record r { a: option<tuple<u8, list<mut s8>>> }",
            "`list<mut s8>`",
        ),
        (
            "record r { a: u32 } func f() -> result<list<r>, list<mut r>> { return result::ok([]); }",
            "`list<mut api::r>`",
        ),
    ];
    for (exported, message) in cases {
        let sources = sources(&[("src/m.wasl", &program(exported))]);
        match analyze(&package(), &sources, &Default::default()) {
            Ok(_) => panic!("accepted: {exported}"),
            Err(e) => {
                assert_eq!(e.code, Code::MutableExport, "{exported}: {}", e.message);
                assert!(e.message.contains(message), "{exported}: {}", e.message);
            }
        }
    }
}

const INVOICE: &str = include_str!("../../examples/invoice/src/invoice.wasl");

/// The target program of v0.2 and its WIT (§ 15.3).
#[test]
fn invoice_example() {
    let sources = sources(&[("src/invoice.wasl", INVOICE)]);
    let package = Package::new("example", "invoice", "0.1.0").unwrap();
    let analysis = match analyze(&package, &sources, &Default::default()) {
        Ok(a) => a,
        Err(e) => panic!("{}", e.display(&sources)),
    };
    let wit = analysis.wit().unwrap();
    let spec = include_str!("../../docs/spec_v0.4/15-examples.md");
    let start = spec.find("package example:invoice@0.1.0;").unwrap();
    let end = start + spec[start..].find("```").unwrap();
    // The printer separates functions with blank lines; the specification does not.
    let lines = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect()
    };
    assert_eq!(lines(&wit), lines(&spec[start..end]));

    // It builds, with an adapter for each exported function (§ 13.8).
    analysis.build("billing").unwrap();
}

/// Invalid programs of v0.2 (§ 15.4): each line in a function of the target program.
#[test]
fn invalid_v2_programs() {
    let cases: &[(&str, Code)] = &[
        ("local s: string = \"abc;", Code::InvalidString),
        ("local s: string = \"\\q\";", Code::InvalidString),
        ("local x: lines = l;", Code::UnknownType),
        ("local t: u32 = inv.total;", Code::UnknownField),
        ("local t: u32 = n.x;", Code::UnknownField),
        ("local t: status = status::lost;", Code::UnknownCase),
        ("local t: status = line::x;", Code::UnknownCase),
        (
            "local m: line = line { quantity: 1, label: \"pen\", unit_price: 5 };",
            Code::RecordFields,
        ),
        (
            "local m: line = line { label: \"pen\", quantity: 1 };",
            Code::RecordFields,
        ),
        (
            "local m: line = line { label: \"pen\", label: \"x\", quantity: 1 };",
            Code::RecordFields,
        ),
        ("local t: status = status::paid();", Code::Payload),
        ("local t: discount = discount::percent;", Code::Payload),
        (
            "match (inv.discount) { case none(x) { } case some { } }",
            Code::Payload,
        ),
        ("local d: u8 = option::none;", Code::TypeMismatch),
        (
            "match (option::none) { case none { } else { } }",
            Code::CannotInfer,
        ),
        ("local m: u32 = list::len([]);", Code::CannotInfer),
        ("local m: u32 = list::len([1, 2]);", Code::CannotInfer),
        ("local s: string;", Code::Uninitialized),
        ("local m: line;", Code::Uninitialized),
        ("local t: tuple<u32, u32>;", Code::Uninitialized),
        ("local r: result<u32, status>;", Code::Uninitialized),
        ("match (n) { case a { } }", Code::NotMatchable),
        ("inv.number = 7;", Code::AssignReadOnly),
        ("inv.lines[0] = l;", Code::AssignReadOnly),
        ("inv.delivery.paper = true;", Code::AssignReadOnly),
        (
            "match (s) { case draft { } case sent { } }",
            Code::NonExhaustive,
        ),
        (
            "match (s) { case draft { } case sent { } case paid { } else { } }",
            Code::RedundantCase,
        ),
        (
            "match (s) { case draft { } case draft { } else { } }",
            Code::RedundantCase,
        ),
        ("match (s) { case lost { } else { } }", Code::UnknownCase),
        ("if (inv == other) { }", Code::InvalidOperator),
        ("if (inv.customer < \"M\") { }", Code::InvalidOperator),
        ("local c: u8 = inv.customer[0];", Code::InvalidOperator),
        (
            "local xs: list<mut u8> = string::bytes(inv.customer);",
            Code::TypeMismatch,
        ),
        (
            "local xs: list<u8> = list::freeze(string::bytes(inv.customer));",
            Code::TypeMismatch,
        ),
        ("local c: char = '';", Code::InvalidCharLiteral),
        ("local c: char = char(n);", Code::InvalidConversion),
        ("local c: u8 = u8('a');", Code::InvalidConversion),
        (
            "local f: delivery = delivery { urgent, email };",
            Code::FlagsOrder,
        ),
        (
            "local f: delivery = delivery { paper, paper };",
            Code::FlagsOrder,
        ),
        ("local f: delivery = delivery { fax };", Code::UnknownCase),
        (
            "local t: tuple<u32, u32> = tuple(1, 2, 3);",
            Code::TypeMismatch,
        ),
        ("local t: u32 = tuple(n, n).2;", Code::UnknownField),
        ("local r: result<u32, status> = result::ok;", Code::Payload),
        ("local r: result<_, status> = result::ok(1);", Code::Payload),
        ("local o: option<u32> = option::some;", Code::Payload),
        (
            "local o: option<u32> = option::maybe(1);",
            Code::UnknownCase,
        ),
        ("result::ok(1);", Code::Syntax),
        ("_ = result::ok(1);", Code::Syntax),
        ("_ = n;", Code::Syntax),
        ("string::len(inv.customer);", Code::IgnoredValue),
        ("total(inv);", Code::IgnoredValue),
        ("_ = probe(inv, other, s, n, l);", Code::TypeMismatch),
        ("inv.status = status::paid;", Code::AssignReadOnly),
        ("local m: u32 = invoices::status;", Code::UnknownCase),
        (
            "local m: u32 = string::nope(inv.customer);",
            Code::UnknownFunc,
        ),
        ("local m: u32 = string::len;", Code::UnknownName),
        (
            "local m: u32 = string::len(inv.customer, n);",
            Code::ArgCount,
        ),
        (
            "local t: discount = discount::percent(1, 2);",
            Code::ArgCount,
        ),
        ("local m: list<u8> = list::copy(d);", Code::TypeMismatch),
    ];
    for (body, code) in cases {
        let program = INVOICE.replace(
            "  func sample() -> invoice {",
            &format!(
                "  func probe(inv: invoice, other: invoice, s: status, n: u32, l: line) {{\n    local d: list<mut u8> = list::filled(4, 0);\n    {body}\n  }}\n\n  func sample() -> invoice {{"
            ),
        );
        let sources = sources(&[("src/invoice.wasl", &program)]);
        match analyze(&package(), &sources, &Default::default()) {
            Ok(_) => panic!("accepted: {body}"),
            Err(e) => assert_eq!(e.code, *code, "{body}: {}", e.display(&sources)),
        }
    }

    let others: &[(&str, Code)] = &[
        ("global s: string = \"a\" + \"b\";", Code::NonConstGlobal),
        (
            "global s: u32 = invoices::count_unpaid([]);",
            Code::NonConstGlobal,
        ),
        (
            "interface p { func f() -> u32 { local l: line = invoices::sample().lines[0]; return 0; } }",
            Code::UnknownType,
        ),
        (
            "interface p { func f() -> list<u8> { local d: list<mut u8> = list::filled(3, 0); return d; } }",
            Code::TypeMismatch,
        ),
    ];
    for (extra, code) in others {
        let program = format!("{INVOICE}\n{extra}\n");
        let sources = sources(&[("src/invoice.wasl", &program)]);
        match analyze(&package(), &sources, &Default::default()) {
            Ok(_) => panic!("accepted: {extra}"),
            Err(e) => assert_eq!(e.code, *code, "{extra}: {}", e.display(&sources)),
        }
    }
}

/// Valid uses of the v0.2 expressions, in a function of the target program.
#[test]
fn valid_v2_bodies() {
    let bodies = [
        "local c: char = 'é'; local k: u32 = u32(c); if (c < 'z' && c != '\\n') { }",
        "local t: tuple<u32, list<char>> = tuple(1, ['a']); local u: char = t.1[0];",
        "local x: tuple<u32, string> = tuple(n, \"a\"); local y: string = x.1 + inv.customer;",
        "local f: delivery = delivery { email, urgent } | inv.delivery & delivery { }; if (f == delivery { } || f.paper) { }",
        "local o: option<option<u32>> = option::some(option::none); local p: option<u32> = option::some(n);",
        "local q: option<u32> = option::some(1);",
        "local r: result = result::ok; local e: result<u32> = result::err; local g: result<_, status> = result::err(status::sent);",
        "local v: discount = invoices::discount::fixed(1); local w: status = invoices::status::paid;",
        "local xs: list<mut u8> = [1, 2]; xs[0] = 3; d[n] = xs[1]; local ys: list<u8> = list::freeze(xs);",
        "local zs: list<mut string> = list::filled(n, \"\"); local ws: list<string> = list::filled(n, \"\"); local m: list<u8> = list::filled(2, 0);",
        "local cs: list<char> = string::chars(inv.customer); local t: string = string::from_chars(cs); local b: option<string> = string::from_bytes(d);",
        "local e: option<char> = char::from_u32(65); local k: u32 = list::len(list::copy(string::chars(inv.customer)));",
        "other = inv; s = other.status;",
        "local t: status; local o: option<line>; local f: delivery; if (t == status::draft && f == delivery { }) { o = option::some(l); }",
        "match (inv.discount) { case some(x) { match (x) { case percent(p) { n = u32(p); } case fixed { } } } case none { } }",
        "while (true) { match (s) { case draft { break; } else { continue; } } }",
        "local g: list<list<u8>> = [[1], [], string::bytes(\"x\")];",
        "local h: list<line> = [l, line { label: \"x\", quantity: 1, unit_price: 2, }];",
        "_ = string::len(inv.customer); _ = total(inv); _ = u32::wrapping_add(n, 1);",
    ];
    for body in bodies {
        let program = INVOICE.replace(
            "  func sample() -> invoice {",
            &format!(
                "  func probe(inv: invoice, other: invoice, s: status, n: u32, l: line) {{\n    local d: list<mut u8> = list::filled(4, 0);\n    {body}\n  }}\n\n  func sample() -> invoice {{"
            ),
        );
        let sources = sources(&[("src/invoice.wasl", &program)]);
        if let Err(e) = analyze(&package(), &sources, &Default::default()) {
            panic!("{body}: {}", e.display(&sources));
        }
    }
    let globals = "global ga: invoices::line = invoices::line { label: \"item\", quantity: 1, unit_price: 0 };
        global gb: option<invoices::discount> = option::none;
        global gc: list<tuple<char, u8>> = [tuple('a', 1)];
        global gd: invoices::delivery = invoices::delivery { paper };
        global ge: result<invoices::discount, u8> = result::ok(invoices::discount::percent(5));
        global gf: mut s8 = -1;";
    let program = format!("{INVOICE}\n{globals}\n");
    let sources = sources(&[("src/invoice.wasl", &program)]);
    if let Err(e) = analyze(&package(), &sources, &Default::default()) {
        panic!("{}", e.display(&sources));
    }
}

/// Every error of a compilation, rendered as the user sees them.
fn all_errors(files: &[(&str, &str)]) -> Vec<String> {
    let sources = sources(files);
    match waslc::analyze(&package(), &sources, &Default::default()) {
        Ok(_) => panic!("accepted: {files:?}"),
        Err(errors) => errors
            .iter()
            .map(|e| e.display(&sources).to_string())
            .collect(),
    }
}

/// Several errors per compilation (§ 11.2).
#[test]
fn several_errors() {
    // One error per function: the second error of `f` is not reported, `g` is analyzed.
    assert_eq!(
        all_errors(&[(
            "src/a.wasl",
            "interface i {\n  func f() -> u32 {\n    local x: u8 = 256;\n    return true;\n  }\n  func g() -> u32 {\n    return y;\n  }\n  func h() -> u32 {\n    return 1;\n  }\n}\n",
        )]),
        [
            "src/a.wasl:3:19: error[E0401]: literal out of range for type `u8` (0 … 255)",
            "src/a.wasl:7:12: error[E0300]: unknown name `y`",
        ]
    );

    // One syntax error per file, in file order; the package is then not analyzed, so
    // the unknown name of `c.wasl` is not reported.
    assert_eq!(
        all_errors(&[
            ("src/a.wasl", "interface i {\n  func f( {\n}\n"),
            (
                "src/b.wasl",
                "interface j {\n  func g() -> u32 { return @; }\n}\n"
            ),
            (
                "src/c.wasl",
                "interface k {\n  func h() -> u32 { return z; }\n}\n"
            ),
        ])
        .len(),
        2
    );

    // An error in the declarations stops the analysis before the bodies.
    let errors = all_errors(&[(
        "src/a.wasl",
        "interface i {\n  func f() -> u32 { return y; }\n  func f() -> u32 { return 1; }\n}\n",
    )]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("E0202"), "{errors:?}");
}
