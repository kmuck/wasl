//! `weasel call`: calls a function exported by a component of the project, with wasmtime.

use wasmtime::component::types::ComponentItem;
use wasmtime::component::{Component, Linker, Type, Val};
use wasmtime::{Engine, Store};

use crate::error::{Error, Result};
use crate::project::Project;

/// A function exported by an interface of a loaded component.
struct Export {
    /// WIT name of the instance exporting the function, e.g. `example:calc/math@0.1.0`.
    instance: String,
    /// WIT name of the function, e.g. `fib-rec`.
    function: String,
    /// Name as written in WaSL, e.g. `math::fib_rec`.
    display: String,
    params: Vec<(String, Type)>,
}

/// Calls `function` (`interface::function`, or `function` if unambiguous) with the given
/// textual arguments. Returns the formatted result, or `None` for a function without one.
///
/// The function is looked up in every world of the project: an interface has a single
/// implementation per package, so every world that exports it runs the same code. Only the
/// component of one of those worlds is built.
pub fn call(project: &Project, function: &str, args: &[String]) -> Result<Option<String>> {
    let analysis = project.analyze()?;
    let (world, display) = resolve(analysis.worlds(), function)?;
    let bytes = project.build(&analysis, world)?;

    let engine = Engine::default();
    let loaded = Component::new(&engine, &bytes).map_err(|e| {
        Error::new(format!(
            "cannot load the component of world `{world}`: {e:#}"
        ))
    })?;
    let exports = exported_functions(&engine, &loaded);
    let export = exports
        .iter()
        .find(|e| e.display == display)
        .ok_or_else(|| {
            Error::new(format!(
                "cannot find `{display}` in the component of world `{world}`"
            ))
        })?;

    if args.len() != export.params.len() {
        return Err(Error::new(format!(
            "`{}` expects {} argument(s) ({}), {} given",
            export.display,
            export.params.len(),
            signature(&export.params),
            args.len()
        )));
    }
    let params = args
        .iter()
        .zip(&export.params)
        .map(|(arg, (name, ty))| {
            parse_value(arg, ty).ok_or_else(|| {
                Error::new(format!(
                    "invalid value `{arg}` for parameter `{name}` of type `{}`",
                    type_name(ty)
                ))
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut store = Store::new(&engine, ());
    let instance = Linker::new(&engine)
        .instantiate(&mut store, &loaded)
        .map_err(|e| Error::new(format!("cannot instantiate world `{world}`: {e:#}")))?;
    let func = instance
        .get_export_index(&mut store, None, &export.instance)
        .and_then(|i| instance.get_export_index(&mut store, Some(&i), &export.function))
        .and_then(|i| instance.get_func(&mut store, i))
        .ok_or_else(|| Error::new(format!("cannot find `{}` in the component", export.display)))?;

    let mut results = vec![Val::Bool(false); func.ty(&store).results().len()];
    if let Err(e) = func.call(&mut store, &params, &mut results) {
        let reason = match e.downcast_ref::<wasmtime::Trap>() {
            Some(trap) => trap
                .to_string()
                .trim_start_matches("wasm trap: ")
                .to_string(),
            None => format!("{e:#}"),
        };
        return Err(Error::new(format!(
            "`{}` trapped: {reason}",
            export.display
        )));
    }
    Ok(results.first().map(format_value))
}

/// Every function exported by an interface of a loaded component.
fn exported_functions(engine: &Engine, component: &Component) -> Vec<Export> {
    let mut exports = Vec::new();
    for (instance, item) in component.component_type().exports(engine) {
        let ComponentItem::ComponentInstance(ty) = item.ty else {
            continue;
        };
        // `example:calc/math@0.1.0` → `math`
        let interface = instance.rsplit('/').next().unwrap_or(instance);
        let interface = interface.split('@').next().unwrap_or(interface);
        for (function, item) in ty.exports(engine) {
            if let ComponentItem::ComponentFunc(func) = item.ty {
                exports.push(Export {
                    instance: instance.to_string(),
                    function: function.to_string(),
                    display: format!("{}::{}", snake(interface), snake(function)),
                    params: func.params().map(|(name, ty)| (snake(name), ty)).collect(),
                });
            }
        }
    }
    exports
}

/// Resolves `interface::function` or `function` among the functions exported by the
/// worlds of the project. Returns a world that exports it, and its full WaSL name.
fn resolve<'a>(worlds: &'a [waslc::World], function: &str) -> Result<(&'a str, String)> {
    // Every exported function, listed once even if several worlds export its interface.
    let mut functions: Vec<(String, &str)> = Vec::new();
    for world in worlds {
        for interface in &world.exports {
            for name in &interface.functions {
                let full = format!("{}::{name}", interface.name);
                if !functions.iter().any(|(f, _)| *f == full) {
                    functions.push((full, &world.name));
                }
            }
        }
    }

    let matches: Vec<&(String, &str)> = if function.contains("::") {
        functions.iter().filter(|(f, _)| f == function).collect()
    } else {
        functions
            .iter()
            .filter(|(f, _)| f.rsplit("::").next() == Some(function))
            .collect()
    };
    let list = |items: &mut dyn Iterator<Item = &String>| {
        items
            .map(|f| format!("`{f}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match matches.as_slice() {
        [(full, world)] => Ok((world, full.clone())),
        [] if functions.is_empty() => Err(Error::new(
            "the project does not export any function: export an interface from a world",
        )),
        [] => Err(Error::new(format!(
            "no exported function `{function}`; exported functions: {}",
            list(&mut functions.iter().map(|(f, _)| f))
        ))),
        several => Err(Error::new(format!(
            "`{function}` is ambiguous, write one of: {}",
            list(&mut several.iter().map(|(f, _)| f))
        ))),
    }
}

/// WIT name → WaSL name: `fib-rec` → `fib_rec`.
fn snake(name: &str) -> String {
    name.replace('-', "_")
}

fn signature(params: &[(String, Type)]) -> String {
    params
        .iter()
        .map(|(name, ty)| format!("{name}: {}", type_name(ty)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// WaSL name of a component type.
fn type_name(ty: &Type) -> String {
    match ty {
        Type::Bool => "bool".into(),
        Type::S8 => "s8".into(),
        Type::S16 => "s16".into(),
        Type::S32 => "s32".into(),
        Type::S64 => "s64".into(),
        Type::U8 => "u8".into(),
        Type::U16 => "u16".into(),
        Type::U32 => "u32".into(),
        Type::U64 => "u64".into(),
        Type::Float32 => "f32".into(),
        Type::Float64 => "f64".into(),
        other => format!("{other:?}"),
    }
}

/// Parses a command-line argument as a value of type `ty`.
fn parse_value(text: &str, ty: &Type) -> Option<Val> {
    Some(match ty {
        Type::Bool => match text {
            "true" => Val::Bool(true),
            "false" => Val::Bool(false),
            _ => return None,
        },
        Type::S8 => Val::S8(text.parse().ok()?),
        Type::S16 => Val::S16(text.parse().ok()?),
        Type::S32 => Val::S32(text.parse().ok()?),
        Type::S64 => Val::S64(text.parse().ok()?),
        Type::U8 => Val::U8(text.parse().ok()?),
        Type::U16 => Val::U16(text.parse().ok()?),
        Type::U32 => Val::U32(text.parse().ok()?),
        Type::U64 => Val::U64(text.parse().ok()?),
        Type::Float32 => Val::Float32(text.parse().ok()?),
        Type::Float64 => Val::Float64(text.parse().ok()?),
        _ => return None,
    })
}

/// Formats a result value.
fn format_value(value: &Val) -> String {
    match value {
        Val::Bool(v) => v.to_string(),
        Val::S8(v) => v.to_string(),
        Val::S16(v) => v.to_string(),
        Val::S32(v) => v.to_string(),
        Val::S64(v) => v.to_string(),
        Val::U8(v) => v.to_string(),
        Val::U16(v) => v.to_string(),
        Val::U32(v) => v.to_string(),
        Val::U64(v) => v.to_string(),
        // Debug formatting always shows a decimal point: `3.0` rather than `3`.
        Val::Float32(v) => format!("{v:?}"),
        Val::Float64(v) => format!("{v:?}"),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        assert_eq!(parse_value("-5", &Type::S8), Some(Val::S8(-5)));
        assert_eq!(parse_value("300", &Type::U8), None);
        assert_eq!(parse_value("-1", &Type::U32), None);
        assert_eq!(parse_value("true", &Type::Bool), Some(Val::Bool(true)));
        assert_eq!(parse_value("2.5", &Type::Float64), Some(Val::Float64(2.5)));
        assert_eq!(format_value(&Val::Float64(3.0)), "3.0");
        assert_eq!(format_value(&Val::U64(u64::MAX)), "18446744073709551615");
    }
}
