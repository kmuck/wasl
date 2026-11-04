//! `weasel init`: creates a new project.

use std::fs;
use std::path::Path;

use waslc::Package;

use crate::error::{Error, Result};
use crate::manifest;

/// Creates the project of `package`, `namespace:name` or `namespace:name@version` as in
/// WIT, in a new directory named after the package name.
pub fn init(package: &str) -> Result<()> {
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
    write(&dir.join(manifest::FILE_NAME), &manifest::render(&package))?;
    write(
        &dir.join("src").join(format!("{world}.wasl")),
        &template(&world),
    )?;

    eprintln!("Created project `{name}` in `{name}/`");
    eprintln!();
    eprintln!("    cd {name}");
    eprintln!("    weasel build {world}");
    eprintln!("    weasel call {world} math::add 1 2");
    Ok(())
}

/// Source of the generated project.
fn template(world: &str) -> String {
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
