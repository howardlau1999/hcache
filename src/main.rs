use dto::InsrtRequest;
use futures::Future;
use hyper::{server::conn::Http, service::service_fn};
use hyper::{Body, Method, Request, Response, StatusCode};
use monoio::net::TcpListener;
use monoio_compat::TcpStreamCompat;
use rocksdb::{Options, WriteBatch, DB};
use std::{cell::RefCell, net::SocketAddr, path::Path, rc::Rc};
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

pub(crate) async fn serve_http<S, F, R, A>(addr: A, mut service: S, db: DB) -> std::io::Result<()>
where
    S: FnMut(Request<Body>, Rc<RefCell<DB>>) -> F + 'static + Copy,
    F: Future<Output = Result<Response<Body>, R>> + 'static,
    R: std::error::Error + 'static + Send + Sync,
    A: Into<SocketAddr>,
{
    let listener = TcpListener::bind(addr.into())?;
    let db = Rc::new(RefCell::new(db));
    loop {
        let (stream, _) = listener.accept().await?;
        let db = db.clone();
        monoio::spawn(Http::new().with_executor(HyperExecutor).serve_connection(
            unsafe { TcpStreamCompat::new(stream) },
            service_fn(move |req| {
                let db = db.clone();
                service(req, db)
            }),
        ));
    }
}

async fn handle_query(key: &str, db: Rc<RefCell<DB>>) -> Result<Response<Body>, hyper::Error> {
    let db = db.borrow_mut();
    let value = db.get(key.as_bytes()).unwrap();
    match value {
        Some(value) => Ok(Response::new(Body::from(value))),
        None => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap()),
    }
}

async fn handle_del(key: &str, db: Rc<RefCell<DB>>) -> Result<Response<Body>, hyper::Error> {
    let db = db.borrow_mut();
    db.delete(key).unwrap();
    Ok(Response::new(Body::from("")))
}

async fn handle_add(
    req: Request<Body>,
    db: Rc<RefCell<DB>>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await.unwrap();
    let dto: dto::InsrtRequest = serde_json::from_slice(&data).unwrap();
    let db = db.borrow_mut();
    db.put(&dto.key, &dto.value).unwrap();
    Ok(Response::new(Body::from("")))
}

async fn handle_batch(
    req: Request<Body>,
    db: Rc<RefCell<DB>>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await.unwrap();
    let dto: Vec<dto::InsrtRequest> = serde_json::from_slice(&data).unwrap();
    let db = db.borrow_mut();
    let mut write_batch = WriteBatch::default();
    for kv in dto {
        write_batch.put(kv.key, kv.value);
    }
    db.write(write_batch).unwrap();
    Ok(Response::new(Body::from("")))
}

async fn handle_list(
    req: Request<Body>,
    db: Rc<RefCell<DB>>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await.unwrap();
    let dto: Vec<String> = serde_json::from_slice(&data).unwrap();
    let dto = &dto;
    let db = db.borrow_mut();
    let values: Result<Vec<InsrtRequest>, ()> = dto
        .into_iter()
        .zip(db.multi_get(dto).into_iter())
        .map(|(key, res)| match res {
            Ok(value) => match value {
                Some(value) => Ok(InsrtRequest {
                    key: key.clone(),
                    value: unsafe { String::from_utf8_unchecked(value) },
                }),
                None => Err(()),
            },
            Err(_) => Err(()),
        })
        .collect();
    match values {
        Ok(kvs) => match serde_json::to_string(&kvs) {
            Ok(json_string) => Ok(Response::new(Body::from(json_string))),
            Err(_) => Ok(Response::builder().status(StatusCode::INTERNAL_SERVER_ERROR).body(Body::empty()).unwrap()),
        },
        Err(_) => Ok(Response::builder().status(StatusCode::NOT_FOUND).body(Body::empty()).unwrap()),
    }
}

async fn hyper_handler(
    req: Request<Body>,
    db: Rc<RefCell<DB>>,
) -> Result<Response<Body>, hyper::Error> {
    if req.method() == &Method::GET {
        let path = req.uri().path();
        let path = path.split("/").collect::<Vec<&str>>();
        return match path[1] {
            "query" => handle_query(path[2], db).await,
            "del" => handle_del(path[2], db).await,
            "zrmv" => Ok(Response::new(Body::from("zrmv"))),
            _ => Ok(Response::new(Body::from("unknown"))),
        };
    } else if req.method() == &Method::POST {
        let path = req.uri().path();
        let path = path.split("/").collect::<Vec<&str>>();
        return match path[1] {
            "add" => handle_add(req, db).await,
            "batch" => handle_batch(req, db).await,
            "list" => handle_list(req, db).await,
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

#[monoio::main]
async fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let db_path = Path::new(&args[1]);
    let mut options = Options::default();
    options.create_if_missing(false);
    let db = DB::open(&options, db_path).unwrap();
    println!("Running http server on 0.0.0.0:23300");
    let _ = serve_http(([0, 0, 0, 0], 23300), hyper_handler, db).await;
    println!("Http server stopped");
}
