mod dto;

use dto::{InsrtRequest, ScoreRange, ScoreValue};
use futures::Future;
use hyper::{server::conn::Http, service::service_fn};
use hyper::{Body, Method, Request, Response, StatusCode};
use monoio::net::TcpListener;
use monoio_compat::TcpStreamCompat;
#[allow(unused)]
use rocksdb::{IteratorMode, Options, WriteBatch, DB};
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
    kv: HashMap<String, String>,
    zsets: HashMap<String, ZSet>,
}

async fn handle_query(
    key: &str,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    let kv = &mut storage.borrow_mut().kv;
    let value = kv.get(key);
    match value {
        Some(value) => Ok(Response::new(Body::from(value.clone()))),
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
    let kv = &mut storage.borrow_mut().kv;
    kv.remove(key);
    Ok(Response::new(Body::empty()))
}

async fn handle_add(
    req: Request<Body>,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await?;
    let dto: dto::InsrtRequest = serde_json::from_slice(&data).unwrap();
    let kv = &mut storage.borrow_mut().kv;
    kv.insert(dto.key, dto.value);
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
            if zset.value_to_score.is_empty() {
                zsets.remove(key);
            }
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
    let kv = &mut storage.borrow_mut().kv;
    for dto in dto {
        kv.insert(dto.key, dto.value);
    }
    Ok(Response::new(Body::empty()))
}

async fn handle_list(
    req: Request<Body>,
    storage: Rc<RefCell<Storage>>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await.unwrap();
    let dto: Vec<String> = serde_json::from_slice(&data).unwrap();
    let kv = &mut storage.borrow_mut().kv;
    let mut kvs = Vec::new();
    for key in dto {
        if let Some(value) = kv.get(&key) {
            kvs.push(InsrtRequest {
                key,
                value: value.clone(),
            });
        } else {
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap());
        }
    }

    match serde_json::to_string(&kvs) {
        Ok(json_string) => Ok(Response::new(Body::from(json_string))),
        Err(_) => Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
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

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let db_path = Path::new(&args[1]);
    let mut options = Options::default();
    options.create_if_missing(true);
    let db = DB::open(&options, db_path).unwrap();
    let mut kv = HashMap::new();
    db.iterator(IteratorMode::Start).for_each(|(key, value)| {
        let key = unsafe { String::from_utf8_unchecked(key.to_vec()) };
        let value = unsafe { String::from_utf8_unchecked(value.to_vec()) };
        kv.insert(key, value);
    });
    let mut rt = monoio::RuntimeBuilder::<monoio::IoUringDriver>::new().enable_all().with_entries(512).build().unwrap();
    println!("Running http server on 0.0.0.0:8080");
    rt.block_on(serve_http(
        ([0, 0, 0, 0], 8080),
        hyper_handler,
        Storage {
            kv,
            zsets: HashMap::new(),
        },
    )).unwrap();
    println!("Http server stopped");
}
