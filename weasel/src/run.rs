//! `weasel run`: runs a world as a command, through its export of `wasi:cli/run` of
//! WASI 0.2 or 0.3 (specification § 14.2).

use std::process::ExitCode;

use wasmtime::Store;
use wasmtime_wasi::{I32Exit, WasiCtx};

use crate::error::{Error, Result};
use crate::host::{self, Host};
use crate::project::Project;

/// Runs the world `world` with the arguments `args`, and returns its exit code.
pub fn run(project: &Project, world: &str, args: &[String]) -> Result<ExitCode> {
    let analysis = project.analyze()?;
    let def = analysis
        .world(world)
        .map_err(|d| Error::diagnostic(&d, &project.sources))?;
    let export = foreign_export(def, "wasi:cli/run")?;
    let wasi_0_2 = export.starts_with("wasi:cli/run@0.2.");

    let engine = host::engine()?;
    let component = host::component(&engine, project, &analysis, world)?;
    let linker = host::linker(&engine)?;
    // The program receives the name of the world first, as a command receives its own
    // name; it has the standard streams of `weasel`, and nothing else.
    let mut argv = vec![world.to_string()];
    argv.extend(args.iter().cloned());
    let ctx = WasiCtx::builder().inherit_stdio().args(&argv).build();

    let ran = host::block_on(async {
        let mut store = Store::new(&engine, Host::new(ctx));
        let instance = linker
            .instantiate_async(&mut store, &component)
            .await
            .map_err(|e| Error::new(format!("cannot instantiate world `{world}`: {e:#}")))?;
        let ran = if wasi_0_2 {
            match wasmtime_wasi::p2::bindings::Command::new(&mut store, &instance) {
                Ok(command) => command.wasi_cli_run().call_run(&mut store).await,
                Err(e) => Err(e),
            }
        } else {
            match wasmtime_wasi::p3::bindings::Command::new(&mut store, &instance) {
                Ok(command) => store
                    .run_concurrent(async |accessor| {
                        let result = command.wasi_cli_run().call_run(accessor).await?;
                        // `run` may go on after it has delivered its result: wait until
                        // its task has ended.
                        std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
                        wasmtime::Result::Ok(result)
                    })
                    .await
                    .and_then(|ran| ran),
                Err(e) => Err(e),
            }
        };
        Ok::<_, Error>(ran)
    })??;
    match ran {
        Ok(Ok(())) => Ok(ExitCode::SUCCESS),
        Ok(Err(())) => Ok(ExitCode::FAILURE),
        // `wasi:cli/exit`.
        Err(e) if e.downcast_ref::<I32Exit>().is_some() => {
            let I32Exit(code) = *e.downcast_ref::<I32Exit>().unwrap();
            Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
        }
        Err(e) => Err(Error::new(format!(
            "`run` trapped: {}",
            host::trap_reason(&e)
        ))),
    }
}

/// The WIT name of the export of `interface` (`wasi:cli/run`) by the world, with its
/// version: `wasi:cli/run@0.3.0`.
pub fn foreign_export<'a>(world: &'a waslc::World, interface: &str) -> Result<&'a str> {
    if let Some(export) = world.implements.iter().find(|i| {
        i.strip_prefix(interface)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('@'))
    }) {
        return Ok(export);
    }
    let exports: Vec<String> = world
        .exports
        .iter()
        .map(|e| e.name.clone())
        .chain(
            world
                .implements
                .iter()
                .map(|i| i.split('@').next().unwrap_or(i).to_string()),
        )
        .map(|e| format!("`{e}`"))
        .collect();
    let name = &world.name;
    Err(Error::new(if exports.is_empty() {
        format!("world `{name}` does not export `{interface}`")
    } else {
        format!(
            "world `{name}` does not export `{interface}`; its exports are {}",
            exports.join(", ")
        )
    }))
}
