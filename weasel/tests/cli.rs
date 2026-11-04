//! End-to-end tests: runs the `weasel` binary in temporary projects.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

struct Run {
    success: bool,
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
    ok(dir.path(), &["init", "hello-world"]);
    let root = dir.path().join("hello-world");
    assert!(root.join("wasl.toml").is_file());
    assert!(root.join("src/hello_world.wasl").is_file());
    assert!(!root.join(".gitignore").exists());

    ok(&root, &["build", "hello_world"]);
    assert!(root.join("build/hello_world.wasm").is_file());
    assert_eq!(ok(&root, &["call", "math::add", "1", "2"]), "3\n");

    let err = fails(dir.path(), &["init", "hello-world"]);
    assert!(err.contains("already exists"), "{err}");
    let err = fails(dir.path(), &["init", "Hello"]);
    assert!(err.contains("invalid package name"), "{err}");
    let err = fails(dir.path(), &["init", "while"]);
    assert!(err.contains("reserved word"), "{err}");
}

#[test]
fn call() {
    let dir = project(&[("src/math.wasl", MATH)]);
    let dir = dir.path();
    assert_eq!(ok(dir, &["call", "math::add", "-5", "2"]), "-3\n");
    assert_eq!(ok(dir, &["call", "add", "40", "2"]), "42\n");
    assert_eq!(ok(dir, &["call", "half", "3"]), "1.5\n");
    assert_eq!(ok(dir, &["call", "half", "4"]), "2.0\n");
    assert_eq!(ok(dir, &["call", "is_zero", "0"]), "true\n");
    assert_eq!(ok(dir, &["call", "math::nothing"]), "");

    assert_eq!(
        fails(dir, &["call", "math::div", "1", "0"]),
        "error: `math::div` trapped: integer divide by zero\n"
    );
    assert_eq!(
        fails(dir, &["call", "math::div", "-128", "-1"]),
        "error: `math::div` trapped: integer overflow\n"
    );
    assert_eq!(
        fails(dir, &["call", "math::add", "1"]),
        "error: `math::add` expects 2 argument(s) (a: s32, b: s32), 1 given\n"
    );
    assert_eq!(
        fails(dir, &["call", "math::div", "200", "1"]),
        "error: invalid value `200` for parameter `a` of type `s8`: invalid value\n"
    );
    let err = fails(dir, &["call", "math::sub", "1", "2"]);
    assert!(err.contains("no exported function `math::sub`"), "{err}");
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
    // `call` finds the function in any component: there is no world to choose.
    assert_eq!(ok(dir, &["call", "math::add", "1", "2"]), "3\n");
    assert_eq!(ok(dir, &["call", "extra::ten"]), "10\n");
    assert_eq!(ok(dir, &["call", "ten"]), "10\n");
    assert_eq!(
        fails(dir, &["call", "add", "1", "2"]),
        "error: `add` is ambiguous, write one of: `math::add`, `extra::add`\n"
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
    assert_eq!(ok(&calc, &["call", "math::fib", "10"]), "55\n");
    assert_eq!(ok(&calc, &["call", "fib_rec", "20"]), "6765\n");
    assert_eq!(ok(&calc, &["call", "math::clamp", "-50", "0", "10"]), "0\n");
}

/// The invoice returned by `invoices::sample`, in WAVE.
const SAMPLE: &str = r#"{number: 42, customer: "ACME", lines: [{label: "pen", quantity: 3, unit-price: 150}, {label: "notebook", quantity: 2, unit-price: 425}], discount: some(percent(10)), delivery: {email, urgent}, status: draft}"#;

#[test]
fn invoice_example() {
    let invoice = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/invoice");
    let call = |args: &[&str]| {
        let mut all = vec!["call"];
        all.extend(args);
        ok(&invoice, &all)
    };
    assert_eq!(call(&["invoices::sample"]), format!("{SAMPLE}\n"));
    assert_eq!(call(&["total", SAMPLE]), "ok(1170)\n");
    assert_eq!(
        call(&["summary", SAMPLE]),
        "\"Weasel & Co: invoice 42 for ACME (draft)\"\n"
    );
    assert_eq!(call(&["initial", SAMPLE]), "some('A')\n");
    assert_eq!(
        call(&["stats", &format!("[{SAMPLE}, {SAMPLE}]")]),
        "(2, 2340)\n"
    );
    assert_eq!(
        call(&["mark_paid", SAMPLE]),
        format!("ok({})\n", SAMPLE.replace("draft", "paid"))
    );
    // An `err` is a result like any other: printed, with success.
    let empty = r#"{number: 1, customer: "B", lines: [], delivery: {}, status: sent}"#;
    assert_eq!(call(&["total", empty]), "err(no-lines)\n");

    assert_eq!(
        fails(&invoice, &["call", "total", "{number: 1}"]),
        "error: invalid value `{number: 1}` for parameter `inv` of type `invoice`: missing field \"customer\"\n"
    );
    assert_eq!(
        fails(&invoice, &["call", "total"]),
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
      return result::err(calc_error::negative);
    }
    if (x > 100) {
      return result::err(calc_error::too_big);
    }
    return result::ok(u32(x));
  }

  func validate(x: s32) -> result<_, calc_error> {
    _ = check(x)?;
    return result::ok;
  }

  func sum(a: s32, b: s32) -> result<u64, calc_error> {
    validate(a)?;
    local s: u32 = check(a)? + check(b)?;
    return result::ok(u64(s));
  }

  func first(xs: list<u32>) -> option<u32> {
    if (list::len(xs) == 0) {
      return option::none;
    }
    return option::some(xs[0]);
  }

  func first_plus_one(xs: list<u32>) -> option<u32> {
    return option::some(first(xs)? + 1);
  }

  func name(n: u32) -> option<string> {
    if (n == 0) {
      return option::none;
    }
    return option::some(\"x\");
  }

  func shout(n: u32) -> option<string> {
    return option::some(name(n)? + \"!\");
  }

  func flag(ok: bool) -> result {
    if (ok) {
      return result::ok;
    }
    return result::err;
  }

  func twice(ok: bool) -> result<u32> {
    flag(ok)?;
    return result::ok(2);
  }
}
",
    )]);
    let dir = dir.path();
    assert_eq!(ok(dir, &["call", "sum", "2", "3"]), "ok(5)\n");
    assert_eq!(ok(dir, &["call", "sum", "-1", "3"]), "err(negative)\n");
    assert_eq!(ok(dir, &["call", "sum", "2", "200"]), "err(too-big)\n");
    assert_eq!(ok(dir, &["call", "validate", "7"]), "ok\n");
    assert_eq!(ok(dir, &["call", "first_plus_one", "[]"]), "none\n");
    assert_eq!(ok(dir, &["call", "first_plus_one", "[4, 9]"]), "some(5)\n");
    assert_eq!(ok(dir, &["call", "shout", "0"]), "none\n");
    assert_eq!(ok(dir, &["call", "shout", "1"]), "some(\"x!\")\n");
    assert_eq!(ok(dir, &["call", "twice", "true"]), "ok(2)\n");
    assert_eq!(ok(dir, &["call", "twice", "false"]), "err\n");
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
    assert_eq!(ok(&app, &["call", "one"]), "1\n");
    assert_eq!(
        fails(&app, &["call", "twice"]),
        "error: `calls::twice` is only exported by world `main`, which imports `demo:lib/i@0.1.0`: `weasel call` cannot provide imports yet\n"
    );

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
