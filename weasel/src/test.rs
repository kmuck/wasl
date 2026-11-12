//! `weasel test`: runs the tests of the test files of the project (§ 3.6, § 14.2).

use std::process::ExitCode;

use wasmtime::Store;
use wasmtime::component::{Component, Val};

use crate::error::{Error, Result};
use crate::host::{self, Host};
use crate::project::Project;

/// Runs every test whose name contains `filter`, each in a new instance, and prints one
/// line per test, then a summary. Fails when a test fails.
pub fn test(project: &mut Project, filter: Option<&str>) -> Result<ExitCode> {
    project.add_tests()?;
    let analysis = project.analyze()?;
    let tests: Vec<(usize, &str)> = analysis
        .tests()
        .enumerate()
        .filter(|(_, name)| filter.is_none_or(|f| name.contains(f)))
        .collect();
    if tests.is_empty() {
        println!("0 passed");
        return Ok(ExitCode::SUCCESS);
    }
    let (bytes, interface) = analysis
        .build_tests()
        .map_err(|d| Error::diagnostic(&d, &project.sources))?;
    let engine = host::engine()?;
    let component = Component::new(&engine, &bytes)
        .map_err(|e| Error::new(format!("cannot load the component of the tests: {e:#}")))?;
    let linker = host::linker(&engine)?;

    let failed = host::block_on(async {
        let mut failed = 0;
        for &(k, name) in &tests {
            let outcome = run_one(&engine, &linker, &component, &interface, k).await;
            match outcome {
                Ok(()) => println!("ok    {name}"),
                Err(message) => {
                    failed += 1;
                    println!("FAIL  {name}: {message}");
                }
            }
        }
        failed
    })?;
    let passed = tests.len() - failed;
    println!();
    if failed == 0 {
        println!("{passed} passed");
        Ok(ExitCode::SUCCESS)
    } else {
        println!("{passed} passed, {failed} failed");
        Ok(ExitCode::FAILURE)
    }
}

/// Runs the test `k` in a new instance: `Err` holds the message of its failure.
async fn run_one(
    engine: &wasmtime::Engine,
    linker: &wasmtime::component::Linker<Host>,
    component: &Component,
    interface: &str,
    k: usize,
) -> std::result::Result<(), String> {
    let mut store = Store::new(engine, Host::quiet());
    let instance = linker
        .instantiate_async(&mut store, component)
        .await
        .map_err(|e| format!("cannot instantiate the tests: {e:#}"))?;
    let func = instance
        .get_export_index(&mut store, None, interface)
        .and_then(|i| instance.get_export_index(&mut store, Some(&i), &format!("t{k}")))
        .and_then(|i| instance.get_func(&mut store, i))
        .ok_or("cannot find the test in the component")?;
    let mut results = [Val::Bool(false)];
    let called = store
        .run_concurrent(async |accessor| {
            func.call_concurrent(accessor, &[], &mut results).await?;
            // An async test may go on after it has delivered its result: wait until its
            // task has ended, as `weasel call` does.
            std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
            wasmtime::Result::Ok(())
        })
        .await
        .and_then(|called| called);
    if let Err(e) = called {
        return Err(format!("trapped: {}", host::trap_reason(&e)));
    }
    match &results[0] {
        Val::Result(Ok(_)) => Ok(()),
        Val::Result(Err(Some(message))) => match message.as_ref() {
            Val::String(message) => Err(message.clone()),
            other => Err(format!("{other:?}")),
        },
        other => Err(format!("unexpected result {other:?}")),
    }
}
