//! The host that runs the components of the project with wasmtime, for `weasel call`,
//! `weasel run` and `weasel serve` (specification § 14.2): WASI 0.2 and 0.3, and
//! `wasi:http` 0.3 without outgoing requests.

use std::future::Future;

use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpHooks, WasiHttpView};

use crate::error::{Error, Result};
use crate::project::Project;

/// The state of the host in the store of an instance.
pub struct Host {
    ctx: WasiCtx,
    http: WasiHttpCtx,
    table: ResourceTable,
    hooks: NoNetwork,
}

impl Host {
    pub fn new(ctx: WasiCtx) -> Host {
        Host {
            ctx,
            http: WasiHttpCtx::new(),
            table: ResourceTable::new(),
            hooks: NoNetwork,
        }
    }

    /// A host whose WASI standard output goes to the standard error of `weasel`, so that
    /// the standard output of `weasel` only receives its own output (§ 14.1).
    pub fn quiet() -> Host {
        Host::new(
            WasiCtx::builder()
                .stdout(std::io::stderr())
                .inherit_stderr()
                .build(),
        )
    }
}

impl WasiView for Host {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for Host {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            hooks: &mut self.hooks,
            table: &mut self.table,
            ctx: &mut self.http,
        }
    }
}

/// The hooks of `wasi:http`: a component has no access to the network, so an outgoing
/// request is denied (§ 14.2).
struct NoNetwork;

impl WasiHttpHooks for NoNetwork {
    fn send_request(
        &mut self,
        _request: hyper::Request<wasmtime_wasi_http::WasiBody>,
        _options: Option<wasmtime_wasi_http::RequestOptions>,
        _fut: Box<dyn Future<Output = std::result::Result<(), wasmtime_wasi_http::Error>> + Send>,
    ) -> Box<
        dyn Future<
                Output = wasmtime_wasi_http::Result<(
                    hyper::Response<wasmtime_wasi_http::WasiBody>,
                    Box<
                        dyn Future<Output = std::result::Result<(), wasmtime_wasi_http::Error>>
                            + Send,
                    >,
                )>,
            > + Send,
    > {
        Box::new(async { Err(wasmtime_wasi_http::Error::HttpRequestDenied) })
    }
}

/// The wasmtime engine, with WasmGC and its prerequisites (§ 14.6), and the async of
/// the Component Model.
pub fn engine() -> Result<Engine> {
    let mut config = Config::new();
    config
        .wasm_reference_types(true)
        .wasm_function_references(true)
        .wasm_gc(true)
        .wasm_component_model_async(true);
    Engine::new(&config).map_err(|e| Error::new(format!("cannot create the engine: {e:#}")))
}

/// The imports that the host provides: WASI 0.2 and 0.3, and `wasi:http` 0.3.
pub fn linker(engine: &Engine) -> Result<Linker<Host>> {
    let failed = |e: wasmtime::Error| Error::new(format!("cannot provide WASI: {e:#}"));
    let mut linker = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(failed)?;
    wasmtime_wasi::p3::add_to_linker(&mut linker).map_err(failed)?;
    wasmtime_wasi_http::p3::add_to_linker(&mut linker).map_err(failed)?;
    Ok(linker)
}

/// The component of `world`, composed with the components that provide its imports
/// (§ 14.2), and compiled by wasmtime.
pub fn component(
    engine: &Engine,
    project: &Project,
    analysis: &waslc::Analysis,
    world: &str,
) -> Result<Component> {
    let bytes = crate::compose::component(project, analysis, world)?;
    Component::new(engine, &bytes).map_err(|e| {
        Error::new(format!(
            "cannot load the component of world `{world}`: {e:#}"
        ))
    })
}

/// Runs a future to completion on a new tokio runtime.
pub fn block_on<F: Future>(future: F) -> Result<F::Output> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::new(format!("cannot start the runtime: {e}")))?;
    Ok(runtime.block_on(future))
}

/// The reason of a trap, for messages: `integer divide by zero`.
pub fn trap_reason(error: &wasmtime::Error) -> String {
    match error.downcast_ref::<wasmtime::Trap>() {
        Some(trap) => trap
            .to_string()
            .trim_start_matches("wasm trap: ")
            .to_string(),
        None => format!("{error:#}"),
    }
}
