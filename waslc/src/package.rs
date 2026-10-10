//! Package identity: `namespace:name@version` (§ 3.2).

use crate::diag::{Code, Diagnostic, Result};

/// Identity of the package being compiled, as declared by the project manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub namespace: String,
    pub name: String,
    pub version: String,
}

impl Package {
    /// Validates and builds a package identity.
    ///
    /// `namespace` and `name` must be lowercase kebab-case WIT identifiers, and `version` a
    /// SemVer version such as `0.1.0`.
    pub fn new(
        namespace: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Package> {
        let package = Package {
            namespace: namespace.into(),
            name: name.into(),
            version: version.into(),
        };
        for (field, value) in [("namespace", &package.namespace), ("name", &package.name)] {
            if !is_wit_id(value) {
                return Err(Diagnostic::without_span(
                    Code::Manifest,
                    format!(
                        "invalid package {field} `{value}`: expected lowercase words separated by `-`, each starting with a letter"
                    ),
                ));
            }
        }
        if !is_semver(&package.version) {
            return Err(Diagnostic::without_span(
                Code::Manifest,
                format!(
                    "invalid package version `{}`: expected a SemVer version such as `0.1.0`",
                    package.version
                ),
            ));
        }
        Ok(package)
    }
}

/// Lowercase kebab-case WIT identifier.
fn is_wit_id(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|word| {
            word.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                && word
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// SemVer version: `MAJOR.MINOR.PATCH`, optionally followed by `-prerelease` or `+build`.
fn is_semver(s: &str) -> bool {
    let core_end = s.find(['-', '+']).unwrap_or(s.len());
    let (core, rest) = s.split_at(core_end);
    let parts: Vec<&str> = core.split('.').collect();
    let numbers_ok = parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes().all(|c| c.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        });
    let rest_ok = rest.is_empty()
        || (rest.len() > 1
            && rest[1..]
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'+')));
    numbers_ok && rest_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid() {
        assert!(Package::new("my-org", "calc", "0.1.0").is_ok());
        assert!(Package::new("ex", "calc2", "1.0.0-alpha.1").is_ok());
    }

    #[test]
    fn invalid() {
        for (namespace, name, version) in [
            ("Ex", "calc", "0.1.0"),
            ("ex", "my_calc", "0.1.0"),
            ("ex", "2calc", "0.1.0"),
            ("", "calc", "0.1.0"),
            ("ex", "calc", "1.0"),
            ("ex", "calc", "01.0.0"),
            ("ex", "calc", "1.0.0-"),
        ] {
            let err = Package::new(namespace, name, version).unwrap_err();
            assert_eq!(err.code, Code::Manifest, "{namespace} {name} {version}");
        }
    }
}
