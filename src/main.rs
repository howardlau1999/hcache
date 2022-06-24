use futures::Future;
use hyper::{server::conn::Http, service::service_fn};
use monoio::net::TcpListener;
use monoio_compat::TcpStreamCompat;
use std::net::SocketAddr;
mod dto;

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

pub(crate) async fn serve_http<S, F, R, A>(addr: A, service: S) -> std::io::Result<()>
where
    S: FnMut(Request<Body>) -> F + 'static + Copy,
    F: Future<Output = Result<Response<Body>, R>> + 'static,
    R: std::error::Error + 'static + Send + Sync,
    A: Into<SocketAddr>,
{
    let listener = TcpListener::bind(addr.into())?;
    loop {
        let (stream, _) = listener.accept().await?;
        monoio::spawn(
            Http::new()
                .with_executor(HyperExecutor)
                .serve_connection(unsafe { TcpStreamCompat::new(stream) }, service_fn(service)),
        );
    }
}

use hyper::{Body, Method, Request, Response, StatusCode};

async fn handle_add(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await.unwrap();
    let dto: dto::InsrtRequest = serde_json::from_slice(&data).unwrap();
    Ok(Response::new(Body::from(format!(
        "{} = {}",
        dto.key, dto.value
    ))))
}
async fn handle_batch(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await.unwrap();
    let dto: Vec<dto::InsrtRequest> = serde_json::from_slice(&data).unwrap();
    Ok(Response::new(Body::from(format!(
        "{} = {}",
        dto[0].key, dto[0].value
    ))))
}

async fn handle_list(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await.unwrap();
    let dto: Vec<String> = serde_json::from_slice(&data).unwrap();
    Ok(Response::new(Body::from(format!("{}", dto[0]))))
}

async fn hyper_handler(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    if req.method() == &Method::GET {
        let path = req.uri().path();
        let path = path.split("/").collect::<Vec<&str>>();
        match path[1] {
            "query" => return Ok(Response::new(Body::from("query"))),
            "del" => return Ok(Response::new(Body::from("del"))),
            "zrmv" => return Ok(Response::new(Body::from("zrmv"))),
            _ => return Ok(Response::new(Body::from("unknown"))),
        }
    } else if req.method() == &Method::POST {
        let path = req.uri().path();
        let path = path.split("/").collect::<Vec<&str>>();
        return match path[1] {
            "add" => handle_add(req).await,
            "batch" => handle_batch(req).await,
            "list" => handle_list(req).await,
            "zadd" => Ok(Response::new(Body::from("zadd"))),
            "zrange" => Ok(Response::new(Body::from("zrange"))),
            _ => Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Not Found"))
                .unwrap()),
        };
    }
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("404 not found"))
        .unwrap())
}

#[monoio::main(threads = 2)]
async fn main() {
    println!("Running http server on 0.0.0.0:23300");
    let _ = serve_http(([0, 0, 0, 0], 23300), hyper_handler).await;
    println!("Http server stopped");
}
