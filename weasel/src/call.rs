//! `weasel call`: calls a function exported by a component of the project, with wasmtime.

use std::fmt::Write;

use wasmtime::component::types::ComponentItem;
use wasmtime::component::{Component, Linker, Type, Val, wasm_wave};
use wasmtime::{Config, Engine, Store};

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
/// arguments, written in WAVE (§ v2.9.1). Returns the result in WAVE, or `None` for a
/// function without one.
///
/// The function is looked up in every world of the project: an interface has a single
/// implementation per package, so every world that exports it runs the same code. Only the
/// component of one of those worlds is built.
pub fn call(project: &Project, function: &str, args: &[String]) -> Result<Option<String>> {
    let analysis = project.analyze()?;
    let (world, display) = resolve(analysis.worlds(), function)?;
    let bytes = project.build(&analysis, world)?;

    let engine = engine()?;
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

    // Parameter types as written in WaSL, for messages.
    let (interface, name) = display.split_once("::").expect("interface::function");
    let wasl_params = analysis.params(interface, name).unwrap_or_default();
    if args.len() != export.params.len() {
        let signature = wasl_params
            .iter()
            .map(|(name, ty)| format!("{name}: {ty}"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::new(format!(
            "`{}` expects {} argument(s) ({signature}), {} given",
            export.display,
            export.params.len(),
            args.len()
        )));
    }
    let params = args
        .iter()
        .zip(&export.params)
        .zip(&wasl_params)
        .map(|((arg, (name, ty)), (_, wasl_ty))| {
            parse_value(arg, ty).map_err(|reason| {
                Error::new(format!(
                    "invalid value `{arg}` for parameter `{name}` of type `{wasl_ty}`: {reason}"
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

/// The wasmtime engine, with WasmGC and its prerequisites (§ v2.9.2).
fn engine() -> Result<Engine> {
    let mut config = Config::new();
    config
        .wasm_reference_types(true)
        .wasm_function_references(true)
        .wasm_gc(true);
    Engine::new(&config).map_err(|e| Error::new(format!("cannot create the engine: {e:#}")))
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

/// Parses a command-line argument, written in WAVE, as a value of type `ty`.
fn parse_value(text: &str, ty: &Type) -> std::result::Result<Val, String> {
    Val::from_wave(ty, text).map_err(|e| {
        // `wasm-wave` ends its messages with the position of the error (`at 0..3`), which
        // is noise for a single argument.
        let reason = e.to_string();
        let reason = match reason.rsplit_once(" at ") {
            Some((reason, span)) if span.contains("..") => reason,
            _ => &reason,
        };
        reason
            .trim_start_matches("error converting Wasm value: ")
            .to_string()
    })
}

/// Formats a result in WAVE, on one line. Floats keep the format of v0.1 (`3.0`, a valid
/// WAVE float), except NaN, written `nan` (§ v2.9.1). Every field of a record is
/// written, `none` included.
fn format_value(value: &Val) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

/// WAVE keywords, written with a `%` when they name a case.
const KEYWORDS: [&str; 8] = ["true", "false", "some", "none", "ok", "err", "inf", "nan"];

fn write_name(out: &mut String, name: &str) {
    if KEYWORDS.contains(&name) {
        out.push('%');
    }
    out.push_str(name);
}

fn write_list<'a>(out: &mut String, open: &str, items: impl Iterator<Item = &'a Val>, close: &str) {
    out.push_str(open);
    for (i, item) in items.enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_value(out, item);
    }
    out.push_str(close);
}

fn write_payload(out: &mut String, payload: Option<&Val>) {
    if let Some(value) = payload {
        out.push('(');
        write_value(out, value);
        out.push(')');
    }
}

fn write_value(out: &mut String, value: &Val) {
    match value {
        // Debug formatting always shows a decimal point: `3.0` rather than `3`.
        Val::Float32(v) if !v.is_nan() => write!(out, "{v:?}").unwrap(),
        Val::Float64(v) if !v.is_nan() => write!(out, "{v:?}").unwrap(),
        Val::List(items) => write_list(out, "[", items.iter(), "]"),
        Val::Tuple(items) => write_list(out, "(", items.iter(), ")"),
        Val::Record(fields) => {
            out.push('{');
            for (i, (name, value)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(name);
                out.push_str(": ");
                write_value(out, value);
            }
            out.push('}');
        }
        Val::Variant(case, payload) => {
            write_name(out, case);
            write_payload(out, payload.as_deref());
        }
        Val::Enum(case) => write_name(out, case),
        Val::Option(None) => out.push_str("none"),
        Val::Option(Some(value)) => {
            out.push_str("some");
            write_payload(out, Some(value));
        }
        Val::Result(Ok(payload)) => {
            out.push_str("ok");
            write_payload(out, payload.as_deref());
        }
        Val::Result(Err(payload)) => {
            out.push_str("err");
            write_payload(out, payload.as_deref());
        }
        Val::Flags(flags) => {
            out.push('{');
            out.push_str(&flags.join(", "));
            out.push('}');
        }
        // Integers, booleans, NaN, chars and strings, with the escapes of WAVE.
        other => {
            out.push_str(&wasm_wave::to_string(other).unwrap_or_else(|_| format!("{other:?}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        let parse = |text, ty| parse_value(text, ty).ok();
        assert_eq!(parse("-5", &Type::S8), Some(Val::S8(-5)));
        assert_eq!(parse("300", &Type::U8), None);
        assert_eq!(parse("-1", &Type::U32), None);
        assert_eq!(parse("true", &Type::Bool), Some(Val::Bool(true)));
        assert_eq!(parse("2.5", &Type::Float64), Some(Val::Float64(2.5)));
        assert_eq!(format_value(&Val::Float64(3.0)), "3.0");
        assert_eq!(format_value(&Val::Float64(f64::NAN)), "nan");
        assert_eq!(format_value(&Val::Float64(f64::NEG_INFINITY)), "-inf");
        assert_eq!(format_value(&Val::U64(u64::MAX)), "18446744073709551615");
        assert_eq!(format_value(&Val::String("a\"b\n".into())), r#""a\"b\n""#);
        assert_eq!(format_value(&Val::Char('\'')), r"'\''");
        assert_eq!(
            format_value(&Val::Record(vec![
                ("label".into(), Val::String("pen".into())),
                ("discount".into(), Val::Option(None)),
            ])),
            r#"{label: "pen", discount: none}"#
        );
        assert_eq!(
            format_value(&Val::Tuple(vec![
                Val::Result(Ok(None)),
                Val::Result(Err(Some(Box::new(Val::Enum("no-lines".into()))))),
                Val::Variant("none".into(), None),
                Val::Flags(vec![]),
                Val::List(vec![Val::Option(Some(Box::new(Val::U8(1))))]),
            ])),
            "(ok, err(no-lines), %none, {}, [some(1)])"
        );
    }
}
