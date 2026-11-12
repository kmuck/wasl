//! `weasel init`: creates a new project.

use std::fs;
use std::path::Path;

use clap::ValueEnum;
use waslc::Package;

use crate::error::{Error, Result};
use crate::manifest;

/// The program a new project starts with (§ 14.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Template {
    /// An interface with a function, called with `weasel call`.
    Lib,
    /// A command of WASI 0.3, run with `weasel run`.
    Cli,
    /// An HTTP service of WASI 0.3, served with `weasel serve`.
    Http,
}

/// Creates the project of `package`, `namespace:name` or `namespace:name@version` as in
/// WIT, in a new directory named after the package name, from `template`.
pub fn init(package: &str, template: Template) -> Result<()> {
    let (id, version) = match package.split_once('@') {
        Some((id, version)) => (id, version),
        None => (package, "0.1.0"),
    };
    let Some((namespace, name)) = id.split_once(':') else {
        return Err(Error::new(format!(
            "invalid package `{package}`: expected `namespace:name`, such as `acme:shop`"
        )));
    };
    let package = Package::new(namespace, name, version).map_err(|d| Error::new(d.message))?;

    // The world is named after the project: `hello-world` → `hello_world`.
    let world = name.replace('-', "_");
    if waslc::lexer::is_reserved(&world) {
        return Err(Error::new(format!(
            "`{name}` cannot be used as a project name: `{world}` is a reserved word in WaSL"
        )));
    }

    let dir = Path::new(name);
    if dir.exists() {
        return Err(Error::new(format!("destination `{name}` already exists")));
    }
    let write = |path: &Path, text: &str| {
        fs::write(path, text)
            .map_err(|e| Error::new(format!("cannot write `{}`: {e}", path.display())))
    };
    fs::create_dir_all(dir.join("src"))
        .map_err(|e| Error::new(format!("cannot create `{name}`: {e}")))?;
    let (dependency, source) = match template {
        Template::Lib => ("", lib(&world)),
        Template::Cli => ("\n[dependencies]\n\"wasi:cli\" = \"0.3.0\"\n", cli(&world)),
        Template::Http => (
            "\n[dependencies]\n\"wasi:http\" = \"0.3.0\"\n",
            http(&world),
        ),
    };
    write(
        &dir.join(manifest::FILE_NAME),
        &(manifest::render(&package) + dependency),
    )?;
    write(&dir.join("src").join(format!("{world}.wasl")), &source)?;

    eprintln!("Created project `{name}` in `{name}/`");
    eprintln!();
    eprintln!("    cd {name}");
    match template {
        Template::Lib => {
            eprintln!("    weasel build {world}");
            eprintln!("    weasel call {world} math::add 1 2");
        }
        Template::Cli => eprintln!("    weasel run {world}"),
        Template::Http => {
            eprintln!("    weasel serve {world} &");
            eprintln!("    curl http://127.0.0.1:8080/hello");
        }
    }
    Ok(())
}

/// Source of the project `lib`: an interface.
fn lib(world: &str) -> String {
    format!(
        r#"// A world describes a component: the interfaces it exports.
world {world} {{
  export math;
}}

// Every function of an exported interface is part of the component's interface.
interface math {{

  func add(a: s32, b: s32) -> s32 {{
    return a + b;
  }}

}}
"#
    )
}

/// Source of the project `cli`: the command of § 15.7.
fn cli(world: &str) -> String {
    format!(
        r#"// A command of WASI 0.3: the world implements `wasi:cli/run`, and writes on the
// standard output through a stream. Run it with `weasel run {world}`.
world {world} {{
  import wasi:cli/types;
  import wasi:cli/stdout;

  export wasi:cli/run {{
    async func run() -> result {{
      local (rx: stream<u8>, tx: stream_writer<u8>) = stream.new();
      local done: future<result<_, types::error_code>> = stdout::write_via_stream(rx);
      stream.write(tx, string.bytes(greeting::text("world")));
      stream.close(tx);
      match (future.read(done)) {{
        case ok {{
          return result.ok;
        }}
        case err {{
          return result.err;
        }}
      }}
    }}
  }}
}}

interface greeting {{
  func text(name: string) -> string {{
    return "Hello, " + name + "!\n";
  }}
}}
"#
    )
}

/// Source of the project `http`: the service of § 15.8.
fn http(world: &str) -> String {
    format!(
        r#"// An HTTP service of WASI 0.3: the world implements `wasi:http/handler`, and answers
// `Hello, <path>!`. Serve it with `weasel serve {world}`.
world {world} {{
  import wasi:http/types;

  export wasi:http/handler {{
    async func handle(request: types::request) -> result<types::response, types::error_code> {{
      local path: string = "/";
      match (request.get_path_with_query()) {{
        case some(p) {{
          path = p;
        }}
        case none {{
        }}
      }}

      local (body: stream<u8>, body_tx: stream_writer<u8>) = stream.new();
      local (trailers: future<result<option<types::trailers>, types::error_code>>,
             trailers_tx: future_writer<result<option<types::trailers>, types::error_code>>) = future.new();
      local (response: types::response, _) =
        types::response::new(types::fields(), option.some(body), trailers);
      // The host sends the response, then the handler writes its body.
      task.return(result.ok(response));

      stream.write(body_tx, string.bytes("Hello, " + path + "!\n"));
      stream.close(body_tx);
      future.write(trailers_tx, result.ok(option.none));
    }}
  }}
}}
"#
    )
}
