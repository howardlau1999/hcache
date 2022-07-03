use futures::Future;
use hyper::{server::conn::Http, service::service_fn};
use hyper::{Body, Request, Response};
use monoio::net::TcpListener;
use monoio_compat::TcpStreamCompat;
use std::{net::SocketAddr, sync::Arc};

use crate::Storage;

#[derive(Clone)]
struct HyperExecutor;

impl<F> hyper::rt::Executor<F> for HyperExecutor
where
    F: Future + 'static,
    F::Output: 'static,
{
    fn execute(&self, fut: F) {
        monoio::spawn(fut);
    }
}

pub(crate) async fn serve_http<S, F, R, A>(
    addr: A,
    mut service: S,
    storage: Arc<Storage>,
) -> std::io::Result<()>
where
    S: FnMut(Request<Body>, Arc<Storage>) -> F + 'static + Copy,
    F: Future<Output = Result<Response<Body>, R>> + 'static,
    R: std::error::Error + 'static + Send + Sync,
    A: Into<SocketAddr>,
{
    let listener = TcpListener::bind(addr.into())?;
    loop {
        let (stream, addr) = listener.accept().await?;
        let storage = storage.clone();
        monoio::spawn(async move {
            if let Err(e) = Http::new()
                .with_executor(HyperExecutor)
                .serve_connection(
                    unsafe { TcpStreamCompat::new(stream) },
                    service_fn(move |req| {
                        let storage = storage.clone();
                        service(req, storage)
                    }),
                )
                .await
            {
                if !e.is_incomplete_message() {
                    eprintln!("{:?} : {}", addr, e)
                }
            }
        });
    }
}
