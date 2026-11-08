//! `weasel serve`: serves a world as an HTTP service, through its export of
//! `wasi:http/handler` of WASI 0.3 (specification § 14.2). Each request gets a new
//! instance, which runs until the task of `handle` has ended.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use futures::channel::oneshot;
use http_body_util::{BodyExt, Empty};
use hyper::server::conn::http1;
use hyper::{Response, StatusCode};
use wasmtime::{AsContextMut, Engine, Store};
use wasmtime_wasi_http::handler::{Prepared, ProxyPre};
use wasmtime_wasi_http::io::TokioIo;
use wasmtime_wasi_http::p3::bindings::ServicePre;
use wasmtime_wasi_http::p3::bindings::http::types::ErrorCode;
use wasmtime_wasi_http::{WasiBody, WasiHttpView};

use crate::error::{Error, Result};
use crate::host::{self, Host};
use crate::project::Project;
use crate::run::foreign_export;

/// What every request needs: the engine and the component, ready to be instantiated.
struct Server {
    engine: Engine,
    pre: ProxyPre<Host>,
}

/// Serves the world `world` on `addr` until `weasel` is interrupted.
pub fn serve(project: &Project, world: &str, addr: SocketAddr) -> Result<()> {
    let analysis = project.analyze()?;
    let def = analysis
        .world(world)
        .map_err(|d| Error::diagnostic(&d, &project.sources))?;
    foreign_export(def, "wasi:http/handler")?;

    let engine = host::engine()?;
    let component = host::component(&engine, project, &analysis, world)?;
    let linker = host::linker(&engine)?;
    let pre = linker
        .instantiate_pre(&component)
        .and_then(ServicePre::new)
        .map_err(|e| Error::new(format!("cannot instantiate world `{world}`: {e:#}")))?;
    let server = Arc::new(Server {
        engine,
        pre: ProxyPre::P3(pre),
    });

    host::block_on(async move {
        let listener =
            listen(addr).map_err(|e| Error::new(format!("cannot listen on {addr}: {e}")))?;
        let interrupted = interrupted()?;
        tokio::pin!(interrupted);
        let local = listener.local_addr().unwrap_or(addr);
        eprintln!("Serving `{world}` on http://{local}");
        loop {
            let stream = tokio::select! {
                () = &mut interrupted => return Ok(()),
                accepted = listener.accept() => match accepted {
                    Ok((stream, _)) => stream,
                    Err(e) => {
                        eprintln!("error: cannot accept a connection: {e}");
                        continue;
                    }
                },
            };
            let server = server.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |request| {
                    let server = server.clone();
                    async move { Ok::<_, Infallible>(server.handle(request).await) }
                });
                // A connection that fails, such as a client that leaves, ends quietly:
                // errors of the component are printed by `handle`.
                _ = http1::Builder::new()
                    .keep_alive(true)
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    })?
}

/// A future that resolves when `weasel` is interrupted (Ctrl-C, `SIGINT`) or asked to
/// terminate (`SIGTERM`). The signals are caught from the call on.
fn interrupted() -> Result<impl Future<Output = ()>> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let catch =
            |kind| signal(kind).map_err(|e| Error::new(format!("cannot catch the signals: {e}")));
        let mut interrupt = catch(SignalKind::interrupt())?;
        let mut terminate = catch(SignalKind::terminate())?;
        Ok(async move {
            tokio::select! {
                _ = interrupt.recv() => {}
                _ = terminate.recv() => {}
            }
        })
    }
    #[cfg(not(unix))]
    {
        Ok(async {
            _ = tokio::signal::ctrl_c().await;
        })
    }
}

/// A listening socket on `addr`, which can be bound again at once after `weasel` stops.
fn listen(addr: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    let socket = match addr {
        SocketAddr::V4(_) => tokio::net::TcpSocket::new_v4()?,
        SocketAddr::V6(_) => tokio::net::TcpSocket::new_v6()?,
    };
    socket.set_reuseaddr(!cfg!(windows))?;
    socket.bind(addr)?;
    socket.listen(1024)
}

impl Server {
    /// Answers one request with a new instance; a trap or an error of `handle` before
    /// the response gives the status 500 (§ 14.2).
    async fn handle(
        self: Arc<Self>,
        request: hyper::Request<hyper::body::Incoming>,
    ) -> Response<WasiBody> {
        let label = format!("{} {}", request.method(), request.uri());
        let request = request.map(|body| body.map_err(Into::into).boxed_unsync());
        let (tx, rx) = oneshot::channel();
        let server = self.clone();
        let instance = tokio::spawn(async move { server.instance(request, tx).await });
        match rx.await {
            Ok(Ok(response)) => {
                // The instance goes on while the body is sent: a trap after the response
                // is printed, and ends the body.
                tokio::spawn(async move {
                    if let Ok(Err(e)) = instance.await {
                        report(&label, &e);
                    }
                });
                response
            }
            Ok(Err(e)) => {
                report(&label, &e);
                internal_error()
            }
            // The instance ended before `handle` was called or returned.
            Err(oneshot::Canceled) => {
                match instance.await {
                    Ok(Err(e)) => report(&label, &e),
                    Ok(Ok(())) => eprintln!("error: request `{label}` received no response"),
                    Err(e) => eprintln!("error: request `{label}` failed: {e}"),
                }
                internal_error()
            }
        }
    }

    /// Runs one instance: calls `handle` with `request`, sends its response on `tx`, then
    /// runs until the task of `handle` has ended, such as when it has written the body.
    async fn instance(
        &self,
        request: hyper::Request<WasiBody>,
        tx: oneshot::Sender<wasmtime::Result<Response<WasiBody>>>,
    ) -> wasmtime::Result<()> {
        let mut store = Store::new(&self.engine, Host::quiet());
        let proxy = self.pre.instantiate_async(&mut store).await?;
        let prepared = Prepared::new(
            store.as_context_mut(),
            &proxy,
            request,
            <Host as WasiHttpView>::http,
            tx,
        )?;
        store
            .run_concurrent(async |accessor| {
                prepared.run(accessor, std::future::pending()).await?;
                std::future::poll_fn(|cx| accessor.poll_no_interesting_tasks(cx)).await;
                wasmtime::Result::Ok(())
            })
            .await?
    }
}

/// Prints the error of a request on standard error.
fn report(label: &str, error: &wasmtime::Error) {
    match error.downcast_ref::<ErrorCode>() {
        // `handle` returned `result::err(e)`.
        Some(code) => eprintln!(
            "error: request `{label}` failed: {}",
            wasmtime_wasi_http::Error::from(code.clone())
        ),
        None => eprintln!(
            "error: request `{label}` trapped: {}",
            host::trap_reason(error)
        ),
    }
}

/// The response to a request that failed before its response: 500, without a body.
fn internal_error() -> Response<WasiBody> {
    let mut response = Response::new(Empty::new().map_err(|e| match e {}).boxed_unsync());
    *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
    response
}
