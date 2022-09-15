use futures_lite::{AsyncRead, AsyncWrite, Future};
use hyper::service::service_fn;
use std::{
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use glommio::{
    enclose,
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};
use hyper::{server::conn::Http, Body, Request, Response};
use std::{io, rc::Rc};
use tokio::io::ReadBuf;

use crate::Storage;

#[derive(Clone)]
struct HyperExecutor;

impl<F> hyper::rt::Executor<F> for HyperExecutor
where
    F: Future + 'static,
    F::Output: 'static,
{
    fn execute(&self, fut: F) {
        glommio::spawn_local(fut).detach();
    }
}

struct HyperStream(pub TcpStream);
impl tokio::io::AsyncRead for HyperStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0)
            .poll_read(cx, buf.initialize_unfilled())
            .map(|n| {
                if let Ok(n) = n {
                    buf.advance(n);
                }
                Ok(())
            })
    }
}

impl tokio::io::AsyncWrite for HyperStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_close(cx)
    }
}

pub async fn serve_http<S, F, R, A>(
    addr: A,
    mut service: S,
    max_connections: usize,
    storage: Arc<Storage>,
) -> io::Result<()>
where
    S: FnMut(Request<Body>, Arc<Storage>) -> F + 'static + Copy,
    F: Future<Output = Result<Response<Body>, R>> + 'static,
    R: std::error::Error + 'static + Send + Sync,
    A: Into<SocketAddr>,
{
    let listener = TcpListener::bind(addr.into())?;
    let conn_control = Rc::new(Semaphore::new(max_connections as _));
    loop {
        match listener.accept().await {
            Err(x) => {
                return Err(x.into());
            }
            Ok(stream) => {
                let addr = stream.local_addr().unwrap();
                let storage = storage.clone();
                stream.set_nodelay(true);
                glommio::spawn_local(enclose!{(conn_control) async move {
                      let _permit = conn_control.acquire_permit(1).await;
                      if let Err(x) = Http::new().with_executor(HyperExecutor).serve_connection(HyperStream(stream), service_fn(|req| service(req, storage.clone()))).await {
                          if !x.is_incomplete_message() {
                              eprintln!("Stream from {:?} failed with error {:?}", addr, x);
                          }
                      }
                  }}).detach();
            }
        }
    }
}
