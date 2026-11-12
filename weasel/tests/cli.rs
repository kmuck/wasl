//! End-to-end tests: runs the `weasel` binary in temporary projects.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

struct Run {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn weasel(dir: &Path, args: &[&str]) -> Run {
    weasel_env(dir, args, &[])
}

/// Runs `weasel` with extra environment variables.
fn weasel_env(dir: &Path, args: &[&str], env: &[(&str, &Path)]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_weasel"))
        .args(args)
        .envs(env.iter().map(|(k, v)| (*k, *v)))
        .current_dir(dir)
        .output()
        .expect("weasel runs");
    Run {
        success: output.status.success(),
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

/// Runs a command that must succeed and returns its standard output.
fn ok(dir: &Path, args: &[&str]) -> String {
    let run = weasel(dir, args);
    assert!(run.success, "weasel {args:?} failed: {}", run.stderr);
    run.stdout
}

/// Runs a command that must fail and returns its error output.
fn fails(dir: &Path, args: &[&str]) -> String {
    let run = weasel(dir, args);
    assert!(!run.success, "weasel {args:?} succeeded: {}", run.stdout);
    run.stderr
}

/// A project made of the given source files.
fn project(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("wasl.toml"),
        "[package]\nnamespace = \"test\"\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    for (path, text) in files {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

const MATH: &str = "
world app {
  export math;
}

interface math {
  func add(a: s32, b: s32) -> s32 { return a + b; }
  func div(a: s8, b: s8) -> s8 { return a / b; }
  func half(x: f64) -> f64 { return x / 2.0; }
  func is_zero(x: u64) -> bool { return x == 0; }
  func nothing() { }
}
";

#[test]
fn init_build_call() {
    let dir = tempfile::tempdir().unwrap();
    ok(dir.path(), &["init", "demo:hello-world"]);
    let root = dir.path().join("hello-world");
    assert!(root.join("wasl.toml").is_file());
    assert!(root.join("src/hello_world.wasl").is_file());
    assert!(!root.join(".gitignore").exists());

    ok(&root, &["build", "hello_world"]);
    assert!(root.join("build/hello_world.wasm").is_file());
    assert_eq!(
        ok(&root, &["call", "hello_world", "math::add", "1", "2"]),
        "3\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("wasl.toml")).unwrap(),
        "[package]\nnamespace = \"demo\"\nname = \"hello-world\"\nversion = \"0.1.0\"\n"
    );

    // The package is written as in WIT, with an optional version (§ 14.2).
    ok(dir.path(), &["init", "acme:shop@1.2.0"]);
    let manifest = fs::read_to_string(dir.path().join("shop/wasl.toml")).unwrap();
    assert!(manifest.contains("namespace = \"acme\""), "{manifest}");
    assert!(manifest.contains("version = \"1.2.0\""), "{manifest}");

    let err = fails(dir.path(), &["init", "demo:hello-world"]);
    assert!(err.contains("already exists"), "{err}");
    assert_eq!(
        fails(dir.path(), &["init", "hello"]),
        "error: invalid package `hello`: expected `namespace:name`, such as `acme:shop`\n"
    );
    let err = fails(dir.path(), &["init", "demo:Hello"]);
    assert!(err.contains("invalid package name"), "{err}");
    let err = fails(dir.path(), &["init", "demo:x@1.0"]);
    assert!(err.contains("invalid package version"), "{err}");
    let err = fails(dir.path(), &["init", "demo:while"]);
    assert!(err.contains("reserved word"), "{err}");
}

#[test]
fn call() {
    let dir = project(&[("src/math.wasl", MATH)]);
    let dir = dir.path();
    assert_eq!(ok(dir, &["call", "app", "math::add", "-5", "2"]), "-3\n");
    assert_eq!(ok(dir, &["call", "app", "math::add", "40", "2"]), "42\n");
    assert_eq!(ok(dir, &["call", "app", "math::half", "3"]), "1.5\n");
    assert_eq!(ok(dir, &["call", "app", "math::half", "4"]), "2.0\n");
    assert_eq!(ok(dir, &["call", "app", "math::is_zero", "0"]), "true\n");
    assert_eq!(ok(dir, &["call", "app", "math::nothing"]), "");

    assert_eq!(
        fails(dir, &["call", "app", "math::div", "1", "0"]),
        "error: `math::div` trapped: integer divide by zero\n"
    );
    assert_eq!(
        fails(dir, &["call", "app", "math::div", "-128", "-1"]),
        "error: `math::div` trapped: integer overflow\n"
    );
    assert_eq!(
        fails(dir, &["call", "app", "math::add", "1"]),
        "error: `math::add` expects 2 argument(s) (a: s32, b: s32), 1 given\n"
    );
    assert_eq!(
        fails(dir, &["call", "app", "math::div", "200", "1"]),
        "error: invalid value `200` for parameter `a` of type `s8`: invalid value\n"
    );
    let err = fails(dir, &["call", "app", "math::sub", "1", "2"]);
    assert!(
        err.starts_with(
            "error: world `app` does not export `math::sub`; its functions are: `math::add`"
        ),
        "{err}"
    );
    assert_eq!(
        fails(dir, &["call", "app", "add", "1", "2"]),
        "error: write the function with its interface: `math::add`\n"
    );
}

#[test]
fn worlds() {
    let dir = project(&[
        ("src/math.wasl", MATH),
        (
            "src/other.wasl",
            "world other { export math; export extra; }
             interface extra {
               func add(a: u8, b: u8) -> u8 { return a + b; }
               func ten() -> u8 { return 10; }
             }",
        ),
    ]);
    let dir = dir.path();
    // `call` runs the component of the world it is given.
    assert_eq!(ok(dir, &["call", "app", "math::add", "1", "2"]), "3\n");
    assert_eq!(ok(dir, &["call", "other", "math::add", "1", "2"]), "3\n");
    assert_eq!(ok(dir, &["call", "other", "extra::ten"]), "10\n");
    assert_eq!(
        fails(dir, &["call", "app", "extra::ten"]),
        "error: world `app` does not export `extra::ten`; its functions are: `math::add`, `math::div`, `math::half`, `math::is_zero`, `math::nothing`\n"
    );
    assert_eq!(
        fails(dir, &["call", "nope", "math::add", "1", "2"]),
        "error[E9001]: unknown world `nope`: the package defines `app`, `other`\n"
    );
    assert_eq!(
        fails(dir, &["call", "other", "add", "1", "2"]),
        "error: write the function with its interface: `math::add`, `extra::add`\n"
    );
    // One world, one build, one component.
    assert_eq!(
        fails(dir, &["build", "nope"]),
        "error[E9001]: unknown world `nope`: the package defines `app`, `other`\n"
    );
    let err = fails(dir, &["build"]);
    assert!(err.contains("<WORLD>"), "{err}");
    ok(dir, &["build", "other"]);
    assert!(dir.join("build/other.wasm").is_file());
    assert!(!dir.join("build/app.wasm").exists());
    ok(dir, &["build", "app"]);
    assert!(dir.join("build/app.wasm").is_file());

    ok(dir, &["clean"]);
    assert!(!dir.join("build").exists());
}

#[test]
fn check_and_wit() {
    let dir = project(&[("src/math.wasl", MATH)]);
    ok(dir.path(), &["check"]);
    let wit = ok(dir.path(), &["wit"]);
    assert!(wit.contains("package test:demo@0.1.0;"), "{wit}");
    assert!(wit.contains("is-zero: func(x: u64) -> bool;"), "{wit}");

    let bad = project(&[(
        "src/bad.wasl",
        "interface t {\n  func f(n: u32) -> u64 {\n    return n;\n  }\n}\n",
    )]);
    let expected =
        "src/bad.wasl:3:12: error[E0400]: mismatched types: expected `u64`, found `u32`\n";
    assert_eq!(fails(bad.path(), &["check"]), expected);
    assert_eq!(fails(bad.path(), &["build", "app"]), expected);
    assert!(!bad.path().join("build").exists());
}

#[test]
fn project_errors() {
    let empty = tempfile::tempdir().unwrap();
    let err = fails(empty.path(), &["build", "app"]);
    assert!(err.contains("could not find `wasl.toml`"), "{err}");

    let no_src = tempfile::tempdir().unwrap();
    fs::write(
        no_src.path().join("wasl.toml"),
        "[package]\nnamespace = \"test\"\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let err = fails(no_src.path(), &["check"]);
    assert!(err.contains("could not find the `src` directory"), "{err}");

    fs::write(
        no_src.path().join("wasl.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .unwrap();
    let err = fails(no_src.path(), &["check"]);
    assert!(err.starts_with("wasl.toml:1:1: error[E0200]"), "{err}");
}

#[test]
fn calc_example() {
    let calc = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/calc");
    let call = |args: &[&str]| {
        let mut all = vec!["call", "calculator"];
        all.extend(args);
        ok(&calc, &all)
    };
    assert_eq!(call(&["math::fib", "10"]), "55\n");
    assert_eq!(call(&["math::fib_rec", "20"]), "6765\n");
    assert_eq!(call(&["math::clamp", "-50", "0", "10"]), "0\n");
}

/// The invoice returned by `invoices::sample`, in WAVE.
const SAMPLE: &str = r#"{number: 42, customer: "ACME", lines: [{label: "pen", quantity: 3, unit-price: 150}, {label: "notebook", quantity: 2, unit-price: 425}], discount: some(percent(10)), delivery: {email, urgent}, status: draft}"#;

#[test]
fn invoice_example() {
    let invoice = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/invoice");
    let call = |args: &[&str]| {
        let mut all = vec!["call", "billing"];
        all.extend(args);
        ok(&invoice, &all)
    };
    assert_eq!(call(&["invoices::sample"]), format!("{SAMPLE}\n"));
    assert_eq!(call(&["invoices::total", SAMPLE]), "ok(1170)\n");
    assert_eq!(
        call(&["invoices::summary", SAMPLE]),
        "\"Weasel & Co: invoice 42 for ACME (draft)\"\n"
    );
    assert_eq!(call(&["invoices::initial", SAMPLE]), "some('A')\n");
    assert_eq!(
        call(&["invoices::stats", &format!("[{SAMPLE}, {SAMPLE}]")]),
        "(2, 2340)\n"
    );
    assert_eq!(
        call(&["invoices::mark_paid", SAMPLE]),
        format!("ok({})\n", SAMPLE.replace("draft", "paid"))
    );
    // An `err` is a result like any other: printed, with success.
    let empty = r#"{number: 1, customer: "B", lines: [], delivery: {}, status: sent}"#;
    assert_eq!(call(&["invoices::total", empty]), "err(no-lines)\n");

    assert_eq!(
        fails(
            &invoice,
            &["call", "billing", "invoices::total", "{number: 1}"]
        ),
        "error: invalid value `{number: 1}` for parameter `inv` of type `invoice`: missing field \"customer\"\n"
    );
    assert_eq!(
        fails(&invoice, &["call", "billing", "invoices::total"]),
        "error: `invoices::total` expects 1 argument(s) (inv: invoice), 0 given\n"
    );
}

/// `?` on `result` and `option` (§ 8.19, § 12.11).
#[test]
fn error_propagation() {
    let dir = project(&[(
        "src/calc.wasl",
        "
world app {
  export calc;
}

interface calc {
  enum calc_error {
    negative,
    too_big,
  }

  func check(x: s32) -> result<u32, calc_error> {
    if (x < 0) {
      return result.err(calc_error::negative);
    }
    if (x > 100) {
      return result.err(calc_error::too_big);
    }
    return result.ok(u32(x));
  }

  func validate(x: s32) -> result<_, calc_error> {
    check(x)?;
    return result.ok;
  }

  func sum(a: s32, b: s32) -> result<u64, calc_error> {
    validate(a)?;
    local s: u32 = check(a)? + check(b)?;
    return result.ok(u64(s));
  }

  func first(xs: list<u32>) -> option<u32> {
    if (list.len(xs) == 0) {
      return option.none;
    }
    return option.some(xs[0]);
  }

  func first_plus_one(xs: list<u32>) -> option<u32> {
    return option.some(first(xs)? + 1);
  }

  func name(n: u32) -> option<string> {
    if (n == 0) {
      return option.none;
    }
    return option.some(\"x\");
  }

  func shout(n: u32) -> option<string> {
    return option.some(name(n)? + \"!\");
  }

  func flag(ok: bool) -> result {
    if (ok) {
      return result.ok;
    }
    return result.err;
  }

  func twice(ok: bool) -> result<u32> {
    flag(ok)?;
    return result.ok(2);
  }
}
",
    )]);
    let dir = dir.path();
    assert_eq!(ok(dir, &["call", "app", "calc::sum", "2", "3"]), "ok(5)\n");
    assert_eq!(
        ok(dir, &["call", "app", "calc::sum", "-1", "3"]),
        "err(negative)\n"
    );
    assert_eq!(
        ok(dir, &["call", "app", "calc::sum", "2", "200"]),
        "err(too-big)\n"
    );
    assert_eq!(ok(dir, &["call", "app", "calc::validate", "7"]), "ok\n");
    assert_eq!(
        ok(dir, &["call", "app", "calc::first_plus_one", "[]"]),
        "none\n"
    );
    assert_eq!(
        ok(dir, &["call", "app", "calc::first_plus_one", "[4, 9]"]),
        "some(5)\n"
    );
    assert_eq!(ok(dir, &["call", "app", "calc::shout", "0"]), "none\n");
    assert_eq!(
        ok(dir, &["call", "app", "calc::shout", "1"]),
        "some(\"x!\")\n"
    );
    assert_eq!(ok(dir, &["call", "app", "calc::twice", "true"]), "ok(2)\n");
    assert_eq!(ok(dir, &["call", "app", "calc::twice", "false"]), "err\n");
}

/// Writes a file, creating its directory.
fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn manifest(name: &str, deps: &str) -> String {
    format!(
        "[package]\nnamespace = \"demo\"\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[dependencies]\n{deps}"
    )
}

const LIB: &str =
    "world w {\n  export i;\n}\n\ninterface i {\n  func f() -> u32 {\n    return 1;\n  }\n}\n";

/// Path dependencies (§ 3.5): a WaSL project and a directory of WIT files.
#[test]
fn path_dependencies() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        &root.join("app/wasl.toml"),
        &manifest(
            "app",
            "\"demo:lib\" = { path = \"../lib\" }\n\"demo:base\" = { path = \"wit/base\" }\n",
        ),
    );
    write(&root.join("app/src/app.wasl"), LIB);
    write(
        &root.join("app/wit/base/base.wit"),
        "package demo:base@1.0.0;\ninterface types {\n  record point { x: u32 }\n}\n",
    );
    write(
        &root.join("lib/wasl.toml"),
        &manifest("lib", "\"demo:base\" = { path = \"../app/wit/base\" }\n"),
    );
    write(&root.join("lib/src/lib.wasl"), LIB);
    let app = root.join("app");
    assert_eq!(weasel(&app, &["check"]).stderr, "No errors in `app`\n");

    // Errors of a dependency are reported in its own files.
    write(
        &root.join("lib/src/lib.wasl"),
        "interface i { func f() -> u32 { return x; } }\n",
    );
    let err = fails(&app, &["check"]);
    assert!(
        err.starts_with("../lib/src/lib.wasl:1:40: error[E0300]"),
        "{err}"
    );
    write(&root.join("lib/src/lib.wasl"), LIB);

    // The name of the dependency must be the name of its package.
    write(
        &root.join("app/wasl.toml"),
        &manifest("app", "\"demo:other\" = { path = \"../lib\" }\n"),
    );
    assert_eq!(
        fails(&app, &["check"]),
        "wasl.toml: error[E0200]: dependency `demo:other` points to `../lib`, whose package is `demo:lib`\n"
    );
    write(
        &root.join("app/wasl.toml"),
        &manifest("app", "\"demo:other\" = { path = \"wit/base\" }\n"),
    );
    let err = fails(&app, &["check"]);
    assert_eq!(
        err,
        "wasl.toml: error[E0200]: dependency `demo:other` contains the package `demo:base@1.0.0`, not `demo:other`\n"
    );
    write(
        &root.join("app/wasl.toml"),
        &manifest("app", "\"demo:base\" = { path = \"nope\" }\n"),
    );
    assert_eq!(
        fails(&app, &["check"]),
        "wasl.toml: error[E0200]: dependency `demo:base` points to `nope`, which does not exist\n"
    );

    // A cycle of path dependencies.
    write(
        &root.join("app/wasl.toml"),
        &manifest("app", "\"demo:lib\" = { path = \"../lib\" }\n"),
    );
    write(
        &root.join("lib/wasl.toml"),
        &manifest("lib", "\"demo:app\" = { path = \"../app\" }\n"),
    );
    assert_eq!(
        fails(&app, &["check"]),
        "../lib/wasl.toml: error[E0200]: dependency cycle: `demo:app` → `demo:lib` → `demo:app`\n"
    );

    // An invalid manifest in a dependency.
    write(&root.join("lib/wasl.toml"), "[package]\nname = \"lib\"\n");
    let err = fails(&app, &["check"]);
    assert!(
        err.starts_with("../lib/wasl.toml:1:1: error[E0200]"),
        "{err}"
    );
}

/// Registry dependencies (§ 3.5), with a local `wkg` registry: fetched, then pinned in
/// `wkg.lock`.
#[test]
fn registry_dependencies() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let registry = root.join("registry");
    let mut resolve = wit_parser::Resolve::default();
    let base = resolve
        .push_str(
            "base.wit",
            "package demo:base@1.0.0; interface types { record point { x: u32 } }",
        )
        .unwrap();
    let greet = resolve
        .push_str(
            "greet.wit",
            "package demo:greet@0.2.6; interface hello { use demo:base/types@1.0.0.{point}; hi: func(p: point) -> string; }",
        )
        .unwrap();
    write(&registry.join("demo/base/1.0.0.wasm"), "");
    fs::write(
        registry.join("demo/base/1.0.0.wasm"),
        wit_component::encode(&resolve, base, false).unwrap(),
    )
    .unwrap();
    write(&registry.join("demo/greet/0.2.6.wasm"), "");
    fs::write(
        registry.join("demo/greet/0.2.6.wasm"),
        wit_component::encode(&resolve, greet, false).unwrap(),
    )
    .unwrap();
    let config = root.join("config");
    write(
        &config.join("wasm-pkg/config.toml"),
        &format!(
            "[namespace_registries]\ndemo = \"local.test\"\n\n[registry.\"local.test\"]\ntype = \"local\"\n\n[registry.\"local.test\".local]\nroot = {:?}\n",
            registry.to_str().unwrap()
        ),
    );
    let cache = root.join("cache");
    let env = [
        ("XDG_CONFIG_HOME", config.as_path()),
        ("XDG_CACHE_HOME", cache.as_path()),
    ];

    let app = root.join("app");
    write(
        &app.join("wasl.toml"),
        &manifest("app", "\"demo:greet\" = \"0.2.6\"\n"),
    );
    write(&app.join("src/app.wasl"), LIB);
    let run = weasel_env(&app, &["check"], &env);
    assert!(run.success, "{}", run.stderr);
    let lock = fs::read_to_string(app.join("wkg.lock")).unwrap();
    assert!(lock.contains("name = \"demo:greet\""), "{lock}");
    assert!(lock.contains("version = \"0.2.6\""), "{lock}");
    assert!(lock.contains("digest = \"sha256:"), "{lock}");

    // A second build uses the lock file and leaves it unchanged.
    let run = weasel_env(&app, &["check"], &env);
    assert!(run.success, "{}", run.stderr);
    assert_eq!(fs::read_to_string(app.join("wkg.lock")).unwrap(), lock);

    // A version that the registry does not have.
    write(
        &app.join("wasl.toml"),
        &manifest("app", "\"demo:greet\" = \"9.9.9\"\n"),
    );
    let run = weasel_env(&app, &["check"], &env);
    assert!(!run.success);
    assert!(
        run.stderr
            .starts_with("error: cannot fetch the dependencies:"),
        "{}",
        run.stderr
    );
}

/// Imports of interfaces of a dependency (§ 2.5).
#[test]
fn imports() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(&root.join("lib/wasl.toml"), &manifest("lib", ""));
    write(&root.join("lib/src/lib.wasl"), LIB);
    write(
        &root.join("app/wasl.toml"),
        &manifest("app", "\"demo:lib\" = { path = \"../lib\" }\n"),
    );
    write(
        &root.join("app/src/app.wasl"),
        "world main {\n  import demo:lib/i;\n  export calls;\n}\n\nworld plain {\n  export util;\n}\n\n\
         interface calls {\n  func twice() -> u32 {\n    return i::f() + i::f();\n  }\n}\n\n\
         interface util {\n  func one() -> u32 {\n    return 1;\n  }\n}\n",
    );
    let app = root.join("app");
    let wit = ok(&app, &["wit"]);
    assert!(
        wit.contains("world main {\n  import demo:lib/i@0.1.0;\n\n  export calls;\n}"),
        "{wit}"
    );
    ok(&app, &["build", "main"]);
    assert!(app.join("build/main.wasm").is_file());
    assert_eq!(ok(&app, &["call", "plain", "util::one"]), "1\n");
    // `call` plugs the component of the world of `lib` that exports `i` (§ 14.2).
    assert_eq!(ok(&app, &["call", "main", "calls::twice"]), "2\n");

    // An interface of the package, provided by another world of the package, whose own
    // imports are provided in turn; WASI comes from the host.
    write(
        &app.join("src/app.wasl"),
        "world main {\n  import counter;\n  export calls;\n}\n\n\
         world store {\n  import demo:lib/i;\n  export counter;\n}\n\n\
         interface calls {\n  func twice() -> u32 {\n    return counter::next() + counter::next();\n  }\n}\n\n\
         global n: mut u32 = 0;\n\n\
         interface counter {\n  func next() -> u32 {\n    n = n + i::f();\n    return n;\n  }\n}\n",
    );
    assert_eq!(ok(&app, &["call", "main", "calls::twice"]), "3\n");

    // An import that no world and no path dependency provides.
    write(
        &root.join("lib/src/lib.wasl"),
        "world w {\n  import i;\n}\n\ninterface i {\n  func f() -> u32 {\n    return 1;\n  }\n}\n",
    );
    assert_eq!(
        fails(&app, &["call", "main", "calls::twice"]),
        "error: no component provides `demo:lib/i@0.1.0`: add it as a path dependency on a WaSL project\n"
    );
    write(&root.join("lib/src/lib.wasl"), LIB);

    // Every package named in the source must be a dependency (E0215).
    write(
        &root.join("app/src/app.wasl"),
        "world main {\n  import demo:other/i;\n}\n",
    );
    assert_eq!(
        fails(&app, &["check"]),
        "src/app.wasl:2:10: error[E0215]: `demo:other` is not a dependency of the package: add `\"demo:other\"` to `[dependencies]` in `wasl.toml`\n"
    );
}

/// One spelling per option (§ 14.1): `--help` and `--version`, no `-h`, `-V` or `help`.
#[test]
fn command_line() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    assert_eq!(
        ok(dir, &["--version"]),
        format!("weasel {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(ok(dir, &["--help"]).contains("--version"));
    assert!(ok(dir, &["call", "--help"]).contains("<WORLD> <FUNCTION>"));
    for args in [&["-V"][..], &["-h"], &["help"], &["build", "-h"]] {
        let run = weasel(dir, args);
        assert_eq!(run.code, Some(2), "weasel {args:?}: {}", run.stderr);
    }
}

/// Several errors per compilation: one per function, the first ten shown (§ 11.2, § 14.1).
#[test]
fn several_errors() {
    let mut source = String::from("world app {\n  export i;\n}\n\ninterface i {\n");
    for n in 0..12 {
        source.push_str(&format!("  func f{n}() -> u32 {{ return x{n}; }}\n"));
    }
    source.push_str("}\n");
    let dir = project(&[("src/app.wasl", &source)]);
    let err = fails(dir.path(), &["check"]);
    let lines: Vec<&str> = err.lines().collect();
    assert_eq!(lines.len(), 11, "{err}");
    assert_eq!(
        lines[0],
        "src/app.wasl:6:29: error[E0300]: unknown name `x0`"
    );
    assert_eq!(
        lines[9],
        "src/app.wasl:15:29: error[E0300]: unknown name `x9`"
    );
    assert_eq!(lines[10], "error: 2 more errors not shown");
}

/// Async functions of the package (§ 14.2): `call` prints the result once the task has
/// ended, and a trap after `task.return` is still an error.
#[test]
fn call_async() {
    let dir = project(&[(
        "src/work.wasl",
        "
world app {
  export work;
}

interface work {
  async func add(a: u32, b: u32) -> u32 {
    return a + b;
  }

  async func twice(s: string) -> string {
    return s + s;
  }

  async func early(n: u32) -> u32 {
    task.return(n * 2);
    local zero: u32 = 0;
    if (n == 0) {
      zero = 1 / zero;
    }
  }

  async func nothing() { }
}
",
    )]);
    let dir = dir.path();
    assert_eq!(ok(dir, &["call", "app", "work::add", "2", "3"]), "5\n");
    assert_eq!(
        ok(dir, &["call", "app", "work::twice", "\"ab\""]),
        "\"abab\"\n"
    );
    assert_eq!(ok(dir, &["call", "app", "work::early", "4"]), "8\n");
    assert_eq!(ok(dir, &["call", "app", "work::nothing"]), "");
    let run = weasel(dir, &["call", "app", "work::early", "0"]);
    assert_eq!(run.code, Some(1));
    assert_eq!(run.stdout, "");
    assert_eq!(
        run.stderr,
        "error: `work::early` trapped: integer divide by zero\n"
    );
}

/// The interfaces of `wasi:cli@0.3.0` that the tests use, as WASI publishes them.
const WASI_CLI: &str = "
package wasi:cli@0.3.0;

interface environment {
  get-arguments: func() -> list<string>;
}

interface exit {
  exit: func(status: result);
}

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

interface stdout {
  use types.{error-code};
  write-via-stream: func(data: stream<u8>) -> future<result<_, error-code>>;
}
";

/// The interface of `wasi:clocks@0.3.0` that `wasi:http/types` uses.
const WASI_CLOCKS: &str = "
package wasi:clocks@0.3.0;

interface types {
  type duration = u64;
}
";

const WASI_HTTP: &str = include_str!("../../waslc/tests/fixtures/wasi_http_0_3_0.wit");

/// A project that depends on the WIT of WASI 0.3 (`wasi:cli` and `wasi:http`), through
/// path dependencies, with the given source file.
fn wasi_project(source: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        &root.join("wasl.toml"),
        &manifest(
            "app",
            "\"wasi:cli\" = { path = \"wit/cli\" }\n\"wasi:http\" = { path = \"wit/http\" }\n",
        ),
    );
    write(&root.join("wit/cli/cli.wit"), WASI_CLI);
    write(&root.join("wit/http/http.wit"), WASI_HTTP);
    write(&root.join("wit/http/deps/clocks.wit"), WASI_CLOCKS);
    write(&root.join("src/app.wasl"), source);
    dir
}

/// A command that prints its arguments, and ends as its first argument asks.
const ARGS: &str = r#"
world args {
  import wasi:cli/types;
  import wasi:cli/stdout;
  import wasi:cli/environment;
  import wasi:cli/exit;

  export wasi:cli/run {
    async func run() -> result {
      local args: list<string> = environment::get_arguments();
      local text: string = "";
      local i: u32 = 0;
      while (i < list.len(args)) {
        text = text + "[" + args[i] + "]";
        i = i + 1;
      }
      local (rx: stream<u8>, tx: stream_writer<u8>) = stream.new();
      local done: future<result<_, types::error_code>> = stdout::write_via_stream(rx);
      stream.write(tx, string.bytes(text + "\n"));
      stream.close(tx);
      future.read(done);
      local first: string = "";
      if (list.len(args) > 1) {
        first = args[1];
      }
      if (first == "fail") {
        return result.err;
      }
      if (first == "exit") {
        exit::exit(result.err);
      }
      local zero: u32 = 0;
      if (first == "trap") {
        zero = 1 / zero;
      }
      return result.ok;
    }
  }
}

world lib {
  export math;
}

interface math {
  func one() -> u32 {
    return 1;
  }
}
"#;

/// `weasel run` (§ 14.2): the arguments, the standard output and the exit code of a
/// command of WASI 0.3.
#[test]
fn run() {
    let dir = wasi_project(ARGS);
    let dir = dir.path();
    // The name of the world comes first; the other arguments are given unchanged.
    assert_eq!(ok(dir, &["run", "args"]), "[args]\n");
    assert_eq!(
        ok(dir, &["run", "args", "a b", "--help", "--", "-x"]),
        "[args][a b][--help][--][-x]\n"
    );
    // Nothing is written to `build/`.
    assert!(!dir.join("build").exists());

    // `result.err` and `wasi:cli/exit` give the exit code 1, without a message.
    for arg in ["fail", "exit"] {
        let run = weasel(dir, &["run", "args", arg]);
        assert_eq!(run.code, Some(1), "{arg}: {}", run.stderr);
        assert_eq!(run.stdout, format!("[args][{arg}]\n"));
        assert_eq!(run.stderr, "");
    }
    let run = weasel(dir, &["run", "args", "trap"]);
    assert_eq!(run.code, Some(1));
    assert_eq!(run.stderr, "error: `run` trapped: integer divide by zero\n");

    assert_eq!(
        fails(dir, &["run", "lib"]),
        "error: world `lib` does not export `wasi:cli/run`; its exports are `math`\n"
    );
    assert!(fails(dir, &["run"]).contains("<WORLD> [ARGUMENTS]..."));
}

/// A service that answers `Hello, <path>!`, traps on `/trap` before its response and
/// on `/late` after it, and fails on `/err`.
const SERVICE: &str = r#"
world service {
  import wasi:http/types;

  export wasi:http/handler {
    async func handle(request: types::request) -> result<types::response, types::error_code> {
      local path: string = "/";
      match (request.get_path_with_query()) {
        case some(p) {
          path = p;
        }
        case none {
        }
      }
      local zero: u32 = 0;
      if (path == "/trap") {
        zero = 1 / zero;
      }
      if (path == "/err") {
        return result.err(types::error_code::internal_error(option.some("boom")));
      }
      local (body: stream<u8>, body_tx: stream_writer<u8>) = stream.new();
      local (trailers: future<result<option<types::trailers>, types::error_code>>,
             trailers_tx: future_writer<result<option<types::trailers>, types::error_code>>) = future.new();
      local (response: types::response, _) =
        types::response::new(types::fields(), option.some(body), trailers);
      task.return(result.ok(response));
      stream.write(body_tx, string.bytes("Hello, "));
      if (path == "/late") {
        zero = 1 / zero;
      }
      stream.write(body_tx, string.bytes(path + "!\n"));
      stream.close(body_tx);
      future.write(trailers_tx, result.ok(option.none));
    }
  }
}

world lib {
  export math;
}

interface math {
  func one() -> u32 {
    return 1;
  }
}
"#;

/// Sends `GET <path>` to `addr` and returns the status and the body of the response.
fn get(addr: &str, path: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    // A body cut by a trap ends the connection without its last chunk.
    _ = stream.read_to_string(&mut response);
    let (head, mut rest) = response.split_once("\r\n\r\n").unwrap();
    let status = head[9..12].parse().unwrap();
    if !head.contains("transfer-encoding: chunked") {
        return (status, rest.to_string());
    }
    let mut body = String::new();
    while let Some((size, after)) = rest.split_once("\r\n") {
        let size = usize::from_str_radix(size, 16).unwrap();
        if size == 0 || after.len() < size {
            break;
        }
        body.push_str(&after[..size]);
        rest = after[size..].trim_start_matches("\r\n");
    }
    (status, body)
}

/// `weasel serve` (§ 14.2): one instance per request, traps and errors, and the end on
/// `SIGTERM`.
#[cfg(unix)]
#[test]
fn serve() {
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;

    let dir = wasi_project(SERVICE);
    let dir = dir.path();
    assert_eq!(
        fails(dir, &["serve", "lib"]),
        "error: world `lib` does not export `wasi:http/handler`; its exports are `math`\n"
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_weasel"))
        .args(["serve", "service", "--addr", "127.0.0.1:0"])
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    stderr.read_line(&mut line).unwrap();
    let addr = line
        .strip_prefix("Serving `service` on http://")
        .unwrap_or_else(|| panic!("{line}"))
        .trim()
        .to_string();

    assert_eq!(get(&addr, "/karim"), (200, "Hello, /karim!\n".to_string()));
    assert_eq!(get(&addr, "/trap"), (500, String::new()));
    assert_eq!(get(&addr, "/err"), (500, String::new()));
    // A trap after the response ends its body.
    assert_eq!(get(&addr, "/late"), (200, "Hello, ".to_string()));
    // Each request has its own instance: the service goes on after a trap.
    assert_eq!(get(&addr, "/again"), (200, "Hello, /again!\n".to_string()));

    // `SIGTERM` ends the service with the exit code 0.
    let killed = Command::new("kill")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(killed.success());
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    let mut rest = String::new();
    stderr.read_to_string(&mut rest).unwrap();
    assert_eq!(
        rest,
        "error: request `GET /trap` trapped: integer divide by zero\n\
         error: request `GET /err` failed: internal error: boom\n\
         error: request `GET /late` trapped: integer divide by zero\n"
    );
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    assert_eq!(stdout, "");
}

/// Replaces the registry dependency of a project made by `weasel init --template` with
/// the WIT of the tests, which needs no network.
fn offline(root: &Path, dependency: &str) {
    let manifest = fs::read_to_string(root.join("wasl.toml")).unwrap();
    let registry = format!("\"wasi:{dependency}\" = \"0.3.0\"\n");
    assert!(
        manifest.ends_with(&format!("\n[dependencies]\n{registry}")),
        "{manifest}"
    );
    let wasi = wasi_project("");
    for dir in ["cli", "http"] {
        let from = wasi.path().join("wit").join(dir);
        for entry in walk(&from) {
            let to = root
                .join("wit")
                .join(dir)
                .join(entry.strip_prefix(&from).unwrap());
            write(&to, &fs::read_to_string(&entry).unwrap());
        }
    }
    let path = format!("\"wasi:{dependency}\" = {{ path = \"wit/{dependency}\" }}\n");
    fs::write(root.join("wasl.toml"), manifest.replace(&registry, &path)).unwrap();
}

/// The files under `dir`.
fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// `weasel init --template cli|http` (§ 14.2): the programs of § 15.7 and § 15.8, with
/// their dependency on WASI 0.3.
#[test]
fn init_templates() {
    let dir = tempfile::tempdir().unwrap();
    let run = weasel(dir.path(), &["init", "demo:greet", "--template", "cli"]);
    assert!(run.success, "{}", run.stderr);
    assert!(
        run.stderr.ends_with("    cd greet\n    weasel run greet\n"),
        "{}",
        run.stderr
    );
    let root = dir.path().join("greet");
    offline(&root, "cli");
    assert_eq!(ok(&root, &["run", "greet"]), "Hello, world!\n");

    let run = weasel(
        dir.path(),
        &["init", "demo:hello-web", "--template", "http"],
    );
    assert!(run.success, "{}", run.stderr);
    assert!(
        run.stderr.contains("    weasel serve hello_web &\n"),
        "{}",
        run.stderr
    );
    let root = dir.path().join("hello-web");
    offline(&root, "http");
    assert_eq!(ok(&root, &["check"]), "");
    // The service is the program of § 15.8, with the name of the project.
    let source = fs::read_to_string(root.join("src/hello_web.wasl")).unwrap();
    let example = include_str!("../../examples/hello-http/src/hello_http.wasl");
    let body = |text: &str| text[text.find("  import wasi:http/types;").unwrap()..].to_string();
    assert_eq!(
        body(&source).replace(
            "      // The host sends the response, then the handler writes its body.\n",
            ""
        ),
        body(example)
    );

    // `lib` is the default; an unknown template lists the three.
    ok(dir.path(), &["init", "demo:plain", "--template", "lib"]);
    assert_eq!(
        fs::read_to_string(dir.path().join("plain/wasl.toml")).unwrap(),
        "[package]\nnamespace = \"demo\"\nname = \"plain\"\nversion = \"0.1.0\"\n"
    );
    let run = weasel(dir.path(), &["init", "demo:other", "--template", "web"]);
    assert_eq!(run.code, Some(2));
    assert!(
        run.stderr.contains("[possible values: lib, cli, http]"),
        "{}",
        run.stderr
    );
    assert!(!dir.path().join("other").exists());
}
