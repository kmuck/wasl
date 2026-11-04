//! Dependencies of a package (§ 3.5): the WIT of the other packages it uses.
//!
//! `waslc` never reads files or contacts registries: the caller fetches each dependency
//! and gives its WIT, as text (the WIT of a WaSL project), as a binary WIT package (what
//! registries hold), or as an already parsed [`Resolve`] (a directory of `.wit` files).

use std::collections::BTreeMap;

use wit_parser::{PackageId, Resolve};

use crate::diag::{Code, Diagnostic, Result};

/// The WIT packages a package depends on, merged into one [`Resolve`].
#[derive(Clone, Default)]
pub struct Dependencies {
    resolve: Resolve,
    /// The packages added by name (`namespace:name`), with their version.
    added: BTreeMap<String, (Option<String>, PackageId)>,
}

impl Dependencies {
    /// Adds a package given as WIT text, such as the WIT of a WaSL project. The packages
    /// it uses must have been added before.
    pub fn add_wit(&mut self, name: &str, path: &str, text: &str) -> Result<()> {
        if self.added.contains_key(name) {
            return Ok(());
        }
        let mut resolve = self.resolve.clone();
        let package = resolve
            .push_str(path, text)
            .map_err(|e| invalid(name, &format!("{e:#}")))?;
        self.resolve = resolve;
        self.record(name, None, package)
    }

    /// Adds a package given as a binary WIT package (`wit_component::encode`), the format
    /// of registries. The packages it uses are included in it.
    pub fn add_wit_binary(&mut self, name: &str, version: &str, bytes: &[u8]) -> Result<()> {
        let (resolve, package) = match wit_component::decode(bytes) {
            Ok(wit_component::DecodedWasm::WitPackage(resolve, package)) => (resolve, package),
            Ok(wit_component::DecodedWasm::Component(..)) => {
                return Err(invalid(name, "it is a component, not a WIT package"));
            }
            Err(e) => return Err(invalid(name, &format!("{e:#}"))),
        };
        self.add_resolve(name, Some(version), resolve, package)
    }

    /// Adds a package already parsed into its own [`Resolve`], such as a directory of
    /// `.wit` files parsed with [`Resolve::push_path`].
    pub fn add_resolve(
        &mut self,
        name: &str,
        version: Option<&str>,
        resolve: Resolve,
        package: PackageId,
    ) -> Result<()> {
        if let Some((existing, _)) = self.added.get(name) {
            return match (existing, version) {
                (Some(a), Some(b)) if a != b => Err(Diagnostic::without_span(
                    Code::Manifest,
                    format!("dependency `{name}` is required at two versions, `{a}` and `{b}`"),
                )),
                _ => Ok(()),
            };
        }
        let remap = self
            .resolve
            .merge(resolve)
            .map_err(|e| invalid(name, &format!("{e:#}")))?;
        let package = remap.packages[package.index()];
        self.record(name, version, package)
    }

    /// Adds every package of `other`: the dependencies of a dependency, before the
    /// dependency itself.
    pub fn include(&mut self, other: &Dependencies) -> Result<()> {
        for (name, (version, _)) in &other.added {
            if let Some((Some(a), _)) = self.added.get(name)
                && let Some(b) = version
                && a != b
            {
                return Err(Diagnostic::without_span(
                    Code::Manifest,
                    format!("dependency `{name}` is required at two versions, `{a}` and `{b}`"),
                ));
            }
        }
        if other.added.keys().all(|name| self.added.contains_key(name)) {
            return Ok(());
        }
        // The packages already present are matched by name and shared.
        let remap = self.resolve.merge(other.resolve.clone()).map_err(|e| {
            Diagnostic::without_span(
                Code::Manifest,
                format!("invalid WIT in the dependencies: {e:#}"),
            )
        })?;
        for (name, (version, package)) in &other.added {
            self.added
                .entry(name.clone())
                .or_insert_with(|| (version.clone(), remap.packages[package.index()]));
        }
        Ok(())
    }

    /// Checks that the package added for `name` is that package, at that version.
    fn record(&mut self, name: &str, version: Option<&str>, package: PackageId) -> Result<()> {
        let found = &self.resolve.packages[package].name;
        let found_name = format!("{}:{}", found.namespace, found.name);
        if found_name != name {
            return Err(Diagnostic::without_span(
                Code::Manifest,
                format!("dependency `{name}` contains the package `{found}`, not `{name}`"),
            ));
        }
        let found_version = found.version.as_ref().map(|v| v.to_string());
        if let Some(version) = version
            && found_version.as_deref() != Some(version)
        {
            return Err(Diagnostic::without_span(
                Code::Manifest,
                format!(
                    "dependency `{name}` is required at version `{version}`, but its WIT declares `{found}`"
                ),
            ));
        }
        self.added.insert(
            name.to_string(),
            (version.map(str::to_string).or(found_version), package),
        );
        Ok(())
    }

    /// The names (`namespace:name`) of the packages added, in alphabetical order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.added.keys().map(String::as_str)
    }

    /// The version of a package added by name, as its WIT declares it.
    pub fn version(&self, name: &str) -> Option<&str> {
        self.added.get(name)?.0.as_deref()
    }

    /// Every WIT package of the dependencies, including those they use.
    pub fn resolve(&self) -> &Resolve {
        &self.resolve
    }

    /// The package added for `name`.
    pub fn package(&self, name: &str) -> Option<PackageId> {
        Some(self.added.get(name)?.1)
    }
}

fn invalid(name: &str, reason: &str) -> Diagnostic {
    Diagnostic::without_span(
        Code::Manifest,
        format!("invalid WIT in dependency `{name}`: {reason}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(wit: &[&str]) -> Vec<u8> {
        let mut resolve = Resolve::default();
        let mut last = None;
        for (i, text) in wit.iter().enumerate() {
            last = Some(resolve.push_str(format!("{i}.wit"), text).unwrap());
        }
        wit_component::encode(&resolve, last.unwrap(), false).unwrap()
    }

    const BASE: &str = "package demo:base@1.0.0; interface types { record point { x: u32 } }";
    const GREET: &str = "package demo:greet@0.2.6;
        interface hello { use demo:base/types@1.0.0.{point}; hi: func(p: point) -> string; }";

    #[test]
    fn binary_packages_bring_what_they_use() {
        let mut deps = Dependencies::default();
        deps.add_wit_binary("demo:greet", "0.2.6", &binary(&[BASE, GREET]))
            .unwrap();
        // Adding a package already included by another one merges it.
        deps.add_wit_binary("demo:base", "1.0.0", &binary(&[BASE]))
            .unwrap();
        assert_eq!(
            deps.names().collect::<Vec<_>>(),
            ["demo:base", "demo:greet"]
        );
        assert_eq!(deps.resolve().packages.len(), 2);
        assert_eq!(deps.version("demo:greet"), Some("0.2.6"));
    }

    #[test]
    fn text_uses_packages_added_before() {
        let mut deps = Dependencies::default();
        deps.add_wit_binary("demo:base", "1.0.0", &binary(&[BASE]))
            .unwrap();
        deps.add_wit(
            "demo:shop",
            "../shop/wasl.toml",
            "package demo:shop@0.1.0;
             interface orders { use demo:base/types@1.0.0.{point}; at: func() -> point; }",
        )
        .unwrap();
        assert_eq!(deps.version("demo:shop"), Some("0.1.0"));
    }

    #[test]
    fn errors() {
        let mut deps = Dependencies::default();
        let e = deps
            .add_wit_binary("demo:other", "1.0.0", &binary(&[BASE]))
            .unwrap_err();
        assert_eq!(e.code, Code::Manifest);
        assert!(
            e.message.contains("contains the package `demo:base@1.0.0`"),
            "{}",
            e.message
        );

        let e = deps
            .add_wit_binary("demo:base", "2.0.0", &binary(&[BASE]))
            .unwrap_err();
        assert!(e.message.contains("version `2.0.0`"), "{}", e.message);

        let e = deps
            .add_wit_binary("demo:base", "1.0.0", b"nope")
            .unwrap_err();
        assert!(
            e.message
                .starts_with("invalid WIT in dependency `demo:base`"),
            "{}",
            e.message
        );

        let e = deps
            .add_wit(
                "demo:shop",
                "x.wit",
                "package demo:shop; interface i { f: func() -> nope; }",
            )
            .unwrap_err();
        assert!(
            e.message
                .starts_with("invalid WIT in dependency `demo:shop`"),
            "{}",
            e.message
        );
    }
}
