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
    let output = Command::new(env!("CARGO_BIN_EXE_weasel"))
        .args(args)
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
        "error: invalid value `200` for parameter `a` of type `s8`\n"
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
