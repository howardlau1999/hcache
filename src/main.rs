mod dto;

use dto::{InsrtRequest, ScoreValue, ScoreRange};
use futures::Future;
use hyper::{server::conn::Http, service::service_fn};
use hyper::{Body, Method, Request, Response, StatusCode};
use monoio::net::TcpListener;
use monoio_compat::TcpStreamCompat;
use rocksdb::{Options, WriteBatch, DB};
use std::collections::HashMap;
use std::collections::{BTreeMap, BTreeSet};
use std::{cell::RefCell, net::SocketAddr, path::Path, rc::Rc};

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
    storage: Storage,
) -> std::io::Result<()>
where
    S: FnMut(Request<Body>, Rc<RefCell<Storage>>) -> F + 'static + Copy,
    F: Future<Output = Result<Response<Body>, R>> + 'static,
    R: std::error::Error + 'static + Send + Sync,
    A: Into<SocketAddr>,
{
    let listener = TcpListener::bind(addr.into())?;
    let storage = Rc::new(RefCell::new(storage));
    loop {
        let (stream, _) = listener.accept().await?;
        let storage = storage.clone();
        monoio::spawn(Http::new().with_executor(HyperExecutor).serve_connection(
            unsafe { TcpStreamCompat::new(stream) },
            service_fn(move |req| {
                let storage = storage.clone();
                service(req, storage)
            }),
        ));
    }
}

pub struct ZSet {
    value_to_score: HashMap<String, u32>,
    score_to_values: BTreeMap<u32, BTreeSet<String>>,
}

pub struct Storage {
    db: DB,
    zsets: HashMap<String, ZSet>,
}

async fn handle_query(
    key: &str,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    let db = &storage.borrow_mut().db;
    let value = db.get(key.as_bytes()).unwrap();
    match value {
        Some(value) => Ok(Response::new(Body::from(value))),
        None => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap()),
    }
}

async fn handle_del(
    key: &str,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    let db = &storage.borrow_mut().db;
    db.delete(key).unwrap();
    Ok(Response::new(Body::empty()))
}

async fn handle_add(
    req: Request<Body>,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await?;
    let dto: dto::InsrtRequest = serde_json::from_slice(&data).unwrap();
    let db = &storage.borrow_mut().db;
    db.put(&dto.key, &dto.value).unwrap();
    Ok(Response::new(Body::empty()))
}

async fn handle_zadd(
    key: &str,
    body: Body,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    let data = hyper::body::to_bytes(body).await?;
    let dto: ScoreValue = serde_json::from_slice(&data).unwrap();
    let mut storage = storage.borrow_mut();
    let zsets = &mut storage.zsets;
    let zset = zsets.entry(key.to_string()).or_insert_with(|| ZSet {
        value_to_score: HashMap::new(),
        score_to_values: BTreeMap::new(),
    });
    let value = dto.value;
    let new_score = dto.score;
    if let Some(score) = zset.value_to_score.get_mut(&value) {
        if *score != new_score {
            // Remove from old score
            zset.score_to_values.entry(*score).and_modify(|values| {
                values.remove(&value);
            });
            // Add to new score
            zset.score_to_values
                .entry(new_score)
                .or_insert_with(BTreeSet::new)
                .insert(value);
            // Modify score
            *score = new_score;
        }
    } else {
        zset.value_to_score.insert(value.clone(), new_score);
        zset.score_to_values
            .entry(new_score)
            .or_insert_with(BTreeSet::new)
            .insert(value);
    }
    Ok(Response::new(Body::empty()))
}

async fn handle_zrange(
    key: &str,
    body: Body,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    let data = hyper::body::to_bytes(body).await?;
    let dto: ScoreRange = serde_json::from_slice(&data).unwrap();
    let mut storage = storage.borrow_mut();
    let zsets = &mut storage.zsets;

    if let Some(zset) = zsets.get_mut(key) {
        let min_score = dto.min_score;
        let max_score = dto.max_score;
        let values: Vec<_> = zset
            .score_to_values
            .range(min_score..=max_score)
            .map(|(score, assoc_values)| {
                assoc_values.into_iter().map(|value| ScoreValue {
                    score: *score,
                    value: value.clone(),
                })
            })
            .flatten()
            .collect();
        match serde_json::to_string(&values) {
            Ok(json) => Ok(Response::new(Body::from(json))),
            Err(_) => Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()),
        }
    } else {
        Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap())
    }
}

async fn handle_zrmv(
    key: &str,
    value: &str,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    let mut storage = storage.borrow_mut();
    let zsets = &mut storage.zsets;
    if let Some(zset) = zsets.get_mut(key) {
        if let Some(score) = zset.value_to_score.remove(value) {
            zset.score_to_values.entry(score).and_modify(|values| {
                values.remove(value);
            });
        }
    }
    Ok(Response::new(Body::empty()))
}

async fn handle_batch(
    req: Request<Body>,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await?;
    let dto: Vec<dto::InsrtRequest> = serde_json::from_slice(&data).unwrap();
    let db = &storage.borrow_mut().db;
    let mut write_batch = WriteBatch::default();
    for kv in dto {
        write_batch.put(kv.key, kv.value);
    }
    db.write(write_batch).unwrap();
    Ok(Response::new(Body::empty()))
}

async fn handle_list(
    req: Request<Body>,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await.unwrap();
    let dto: Vec<String> = serde_json::from_slice(&data).unwrap();
    let dto = &dto;
    let db = &storage.borrow_mut().db;
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
            Err(_) => Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()),
        },
        Err(_) => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap()),
    }
}

async fn hyper_handler(
    req: Request<Body>,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    if req.method() == &Method::GET {
        let path = req.uri().path();
        let path = path.splitn(4, "/").collect::<Vec<&str>>();
        return match path[1] {
            "query" => handle_query(path[2], storage).await,
            "del" => handle_del(path[2], storage).await,
            "zrmv" => handle_zrmv(path[2], path[3], storage).await,
            "init" => Ok(Response::new(Body::from("ok"))),
            _ => Ok(Response::new(Body::from("unknown"))),
        };
    } else if req.method() == &Method::POST {
        let path = req.uri().path().to_string();
        let path = path.splitn(3, "/").collect::<Vec<&str>>();
        return match path[1] {
            "add" => handle_add(req, storage).await,
            "batch" => handle_batch(req, storage).await,
            "list" => handle_list(req, storage).await,
            "zadd" => handle_zadd(path[2], req.into_body(), storage).await,
            "zrange" => handle_zrange(path[2], req.into_body(), storage).await,
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
    options.create_if_missing(true);
    let db = DB::open(&options, db_path).unwrap();
    println!("Running http server on 0.0.0.0:8080");
    let _ = serve_http(
        ([0, 0, 0, 0], 8080),
        hyper_handler,
        Storage {
            db,
            zsets: HashMap::new(),
        },
    )
    .await;
    println!("Http server stopped");
}
