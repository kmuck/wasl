//! The project manifest, `wasl.toml`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;
use waslc::Package;
use waslc::source::SourceFile;

use crate::error::{Error, Result};

pub const FILE_NAME: &str = "wasl.toml";

/// Error code of manifest errors (specification § 11.4).
const CODE: &str = "E0200";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestToml {
    package: PackageSection,
    #[serde(default)]
    dependencies: BTreeMap<String, DependencyToml>,
}

/// A dependency: `"0.2.6"` or `{ path = "../catalog" }` (§ 3.5).
#[derive(Deserialize)]
#[serde(untagged)]
enum DependencyToml {
    Version(String),
    Path(PathTable),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathTable {
    path: String,
}

/// The content of `wasl.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub package: Package,
    /// The dependencies, sorted by name.
    pub dependencies: Vec<Dependency>,
}

/// A dependency of the package (§ 3.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// `namespace:name`.
    pub name: String,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// An exact version, fetched from a registry.
    Registry(String),
    /// A WaSL project or a directory of WIT files, relative to the root of the project.
    Path(PathBuf),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageSection {
    namespace: String,
    name: String,
    version: String,
}

/// Reads `wasl.toml`.
pub fn parse(text: &str) -> Result<Manifest> {
    let manifest: ManifestToml = toml::from_str(text).map_err(|e| {
        let location = match e.span() {
            Some(span) => {
                let file = SourceFile::new(FILE_NAME.into(), text.to_string());
                let (line, col) = file.line_col(span.start as u32);
                format!("{FILE_NAME}:{line}:{col}")
            }
            None => FILE_NAME.to_string(),
        };
        Error::rendered(format!("{location}: error[{CODE}]: {}", e.message()))
    })?;
    let p = manifest.package;
    let package = Package::new(p.namespace, p.name, p.version).map_err(|d| invalid(&d.message))?;
    let mut dependencies = Vec::new();
    for (name, dep) in manifest.dependencies {
        let Some((namespace, package_name)) = name.split_once(':') else {
            return Err(invalid(&format!(
                "invalid dependency `{name}`: expected a package name `namespace:name`"
            )));
        };
        let version = match &dep {
            DependencyToml::Version(v) => v.as_str(),
            DependencyToml::Path(_) => "0.0.0",
        };
        Package::new(namespace, package_name, "0.0.0").map_err(|_| {
            invalid(&format!(
                "invalid dependency `{name}`: expected a package name `namespace:name`, in lowercase words separated by `-`"
            ))
        })?;
        Package::new(namespace, package_name, version).map_err(|_| {
            invalid(&format!(
                "invalid version `{version}` for dependency `{name}`: expected an exact version such as `0.2.6`"
            ))
        })?;
        let source = match dep {
            DependencyToml::Version(v) => Source::Registry(v),
            DependencyToml::Path(t) => Source::Path(PathBuf::from(t.path)),
        };
        dependencies.push(Dependency { name, source });
    }
    Ok(Manifest {
        package,
        dependencies,
    })
}

fn invalid(message: &str) -> Error {
    Error::rendered(format!("{FILE_NAME}: error[{CODE}]: {message}"))
}

/// Text of the manifest of a package.
pub fn render(package: &Package) -> String {
    format!(
        "[package]\nnamespace = \"{}\"\nname = \"{}\"\nversion = \"{}\"\n",
        package.namespace, package.name, package.version
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let package = Package::new("example", "calc", "0.1.0").unwrap();
        assert_eq!(parse(&render(&package)).unwrap().package, package);
    }

    #[test]
    fn dependencies() {
        let manifest = parse(
            "[package]\nnamespace = \"ex\"\nname = \"shop\"\nversion = \"0.1.0\"\n\n[dependencies]\n\"wasi:cli\" = \"0.2.6\"\n\"ex:catalog\" = { path = \"../catalog\" }\n",
        )
        .unwrap();
        assert_eq!(
            manifest.dependencies,
            [
                Dependency {
                    name: "ex:catalog".into(),
                    source: Source::Path("../catalog".into()),
                },
                Dependency {
                    name: "wasi:cli".into(),
                    source: Source::Registry("0.2.6".into()),
                },
            ]
        );
        let err = |deps: &str| {
            parse(&format!(
                "[package]\nnamespace = \"ex\"\nname = \"shop\"\nversion = \"0.1.0\"\n[dependencies]\n{deps}\n"
            ))
            .unwrap_err()
            .to_string()
        };
        assert_eq!(
            err("\"wasi:cli\" = \"^0.2\""),
            "wasl.toml: error[E0200]: invalid version `^0.2` for dependency `wasi:cli`: expected an exact version such as `0.2.6`"
        );
        assert_eq!(
            err("cli = \"0.2.6\""),
            "wasl.toml: error[E0200]: invalid dependency `cli`: expected a package name `namespace:name`"
        );
        assert!(err("\"Wasi:cli\" = \"0.2.6\"").contains("invalid dependency `Wasi:cli`"));
        assert!(err("\"a:b\" = { path = \"x\", version = \"1.0.0\" }").starts_with("wasl.toml:6:"));
    }

    #[test]
    fn errors() {
        let err = |text: &str| parse(text).unwrap_err().to_string();
        assert!(
            err("[package]\nnamespace = \"ex\"\nname = \"calc\"\n")
                .starts_with("wasl.toml:1:1: error[E0200]")
        );
        assert!(
            err("[package]\nnamespace = \"ex\"\nname = \"calc\"\nversion = \"0.1.0\"\nauthor = \"me\"\n")
                .starts_with("wasl.toml:5:1: error[E0200]")
        );
        assert_eq!(
            err("[package]\nnamespace = \"ex\"\nname = \"My_Calc\"\nversion = \"0.1.0\"\n"),
            "wasl.toml: error[E0200]: invalid package name `My_Calc`: expected lowercase words separated by `-`, each starting with a letter"
        );
    }
}
