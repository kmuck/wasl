//! Compilation of valid and invalid programs (specification, chapter 15).

use waslc::diag::Code;
use waslc::source::SourceMap;
use waslc::{Analysis, Package, analyze};

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
    analyze(&package(), &sources).map_err(|e| e.display(&sources).to_string())
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

/// Invalid programs from the specification (§ 15.3) and their expected error code.
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
        ("local q: u32 = 1 @ 2;", Code::InvalidChar),
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
        match analyze(&package(), &sources) {
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
    assert!(global_init.contains("E0100"), "{global_init}");
}
