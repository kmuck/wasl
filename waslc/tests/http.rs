//! The HTTP target program of 0.4, `examples/hello-http` (specification, § 15.8), built
//! against the WIT of `wasi:http@0.3.0`, and the WIT names with uppercase letters that
//! this WIT has (§ 13.2). The program runs with `weasel serve` in CI.

use waslc::{Dependencies, Package, SourceMap, diag::Code};

/// The interface of `wasi:clocks@0.3.0` that `wasi:http/types` uses, as WASI publishes it.
const WASI_CLOCKS: &str = r#"
package wasi:clocks@0.3.0;

interface types {
  type duration = u64;
}
"#;

const WASI_HTTP: &str = include_str!("fixtures/wasi_http_0_3_0.wit");

const HELLO_HTTP: &str = include_str!("../../examples/hello-http/src/hello_http.wasl");

fn deps() -> Dependencies {
    let mut deps = Dependencies::default();
    deps.add_wit("wasi:clocks", "clocks.wit", WASI_CLOCKS)
        .unwrap();
    deps.add_wit("wasi:http", "http.wit", WASI_HTTP).unwrap();
    deps
}

fn sources(source: &str) -> SourceMap {
    let mut sources = SourceMap::default();
    sources.add("src/main.wasl".into(), source.to_string());
    sources
}

fn analyze(source: &str) -> waslc::Result<waslc::Analysis> {
    let package = Package::new("example", "hello-http", "0.1.0").unwrap();
    waslc::analyze(&package, &sources(source), &deps()).map_err(|mut e| e.remove(0))
}

fn build(source: &str, world: &str) -> Vec<u8> {
    analyze(source)
        .and_then(|analysis| analysis.build(world))
        .unwrap_or_else(|e| panic!("{}", e.display(&sources(source))))
}

fn validate(bytes: &[u8]) {
    let mut features = wasmparser::WasmFeatures::default();
    features.insert(wasmparser::WasmFeatures::GC);
    features.insert(wasmparser::WasmFeatures::CM_ASYNC);
    wasmparser::Validator::new_with_features(features)
        .validate_all(bytes)
        .unwrap();
}

#[test]
fn hello_http() {
    validate(&build(HELLO_HTTP, "hello_http"));
    // `weasel serve` finds the handler among the implementations of the world (§ 14.3).
    let analysis = analyze(HELLO_HTTP).unwrap();
    let world = analysis.world("hello_http").unwrap();
    assert_eq!(world.implements, ["wasi:http/handler@0.3.0"]);
    assert!(world.exports.is_empty());
}

#[test]
fn hello_http_wit() {
    // The WIT of § 15.8.
    let wit = analyze(HELLO_HTTP).unwrap().wit().unwrap();
    let spec = include_str!("../../docs/spec_v0.4/15-examples.md");
    let section = &spec[spec.find("## 15.8").unwrap()..];
    let start = section.find("package example:hello-http@0.1.0;").unwrap();
    let end = start + section[start..].find("```").unwrap();
    let lines = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect()
    };
    assert_eq!(lines(&wit), lines(&section[start..end]));
}

/// A handler that answers the scheme of the request and the error `DNS-timeout`.
const SCHEMES: &str = r#"
world schemes {
  import wasi:http/types;
  export wasi:http/handler {
    async func handle(request: types::request) -> result<types::response, types::error_code> {
      match (request.get_scheme()) {
        case some(scheme) {
          match (scheme) {
            case http { }
            case https {
              return result::err(types::error_code::tls_protocol_error);
            }
            case other(name) {
              return result::err(types::error_code::dns_error(
                types::dns_error_payload { rcode: option::some(name), info_code: option::none }));
            }
          }
        }
        case none {
          return result::err(types::error_code::dns_timeout);
        }
      }
      return result::err(types::error_code::http_request_uri_invalid);
    }
  }
  export uses;
}

interface uses {
  func payload(code: u16) -> types::dns_error_payload {
    return types::dns_error_payload { rcode: option::none, info_code: option::some(code) };
  }
}
"#;

#[test]
fn uppercase_names() {
    // `HTTP`, `DNS-timeout` and `DNS-error-payload` are written in lowercase (§ 13.2)…
    validate(&build(SCHEMES, "schemes"));
    // … and the WIT keeps the spelling of the dependency.
    let wit = analyze(SCHEMES).unwrap().wit().unwrap();
    assert!(
        wit.contains("use wasi:http/types@0.3.0.{DNS-error-payload};"),
        "{wit}"
    );
    assert!(
        wit.contains("payload: func(code: u16) -> DNS-error-payload;"),
        "{wit}"
    );
    // The uppercase spelling is not a WaSL name.
    let upper = SCHEMES.replace("error_code::dns_timeout", "error_code::DNS_timeout");
    match analyze(&upper) {
        Ok(_) => panic!("accepted"),
        Err(e) => assert_eq!(e.code, Code::InvalidIdent, "{}", e.message),
    }
}

#[test]
fn invalid_handlers() {
    // The lines of § 15.9 on `task::return`, in `handle`.
    let delivery = "task::return(result::ok(response));";
    let close = "stream::close(body_tx);";
    let cases = [
        (
            delivery,
            "if (path == \"/\") { task::return(result::ok(response)); }",
            Code::TaskReturnPlace,
        ),
        (
            delivery,
            "task::return(result::ok(response));\n task::return(result::err(types::error_code::internal_error(option::none)));",
            Code::TaskReturnPlace,
        ),
        (
            close,
            "stream::close(body_tx);\n return result::err(types::error_code::internal_error(option::none));",
            Code::AfterTaskReturn,
        ),
        (
            close,
            "stream::close(body_tx);\n local d: u32 = uses::digits(path)?;",
            Code::AfterTaskReturn,
        ),
    ];
    for (line, replacement, code) in cases {
        let source = HELLO_HTTP.replace(line, replacement)
            + "interface uses { func digits(s: string) -> result<u32, types::error_code> { return result::ok(0); } }";
        assert_ne!(source, HELLO_HTTP);
        match analyze(&source) {
            Ok(_) => panic!("accepted: {replacement}"),
            Err(e) => assert_eq!(e.code, code, "{replacement}: {}", e.message),
        }
    }
}
