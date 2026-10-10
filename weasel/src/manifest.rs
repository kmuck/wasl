//! The project manifest, `wasl.toml`.

use serde::Deserialize;
use waslc::Package;
use waslc::source::SourceFile;

use crate::error::{Error, Result};

pub const FILE_NAME: &str = "wasl.toml";

/// Error code of manifest errors (specification § 11.4).
const CODE: &str = "E0200";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    package: PackageSection,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PackageSection {
    namespace: String,
    name: String,
    version: String,
}

/// Reads the package identity from the text of `wasl.toml`.
pub fn parse(text: &str) -> Result<Package> {
    let manifest: Manifest = toml::from_str(text).map_err(|e| {
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
    Package::new(p.namespace, p.name, p.version).map_err(|d| {
        Error::rendered(format!(
            "{FILE_NAME}: error[{}]: {}",
            d.code.as_str(),
            d.message
        ))
    })
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
        assert_eq!(parse(&render(&package)).unwrap(), package);
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
