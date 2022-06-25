mod dto;
mod http_parser;

use dto::{InsrtRequest, ScoreRange, ScoreValue};
use futures::Future;
use hyper::{server::conn::Http, service::service_fn};
use hyper::{Body, Method, Request, Response, StatusCode};
use monoio::io::AsyncReadRent;
use monoio::net::{TcpListener, TcpStream};
use monoio_compat::TcpStreamCompat;
use parking_lot::RwLock;
use rocksdb::{DBWithThreadMode, MultiThreaded, Options, WriteBatch};
use std::collections::HashMap;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::{net::SocketAddr, path::Path};

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

enum HttpMethod {
    Unknown,
    Get,
    Post,
}

enum HttpRequestParserState {
    ParseMethod,
    SkipSpace,
    ParsePath,
    SkipHeader,
    ParseBody,
}

struct ParserContext {
    buf: Vec<u8>,
    method_pos: usize,
    state: HttpRequestParserState,
    method: HttpMethod,
    method_buf: [u8; 4],
    cur_path_component: Vec<u8>,
    path: Vec<String>,
    rnrn_pos: usize,
    
}

impl ParserContext {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            method_pos: 0,
            state: HttpRequestParserState::ParseMethod,
            method: HttpMethod::Unknown,
            path: Vec::new(),
            cur_path_component: Vec::new(),
            method_buf: [0; 4],
            rnrn_pos: 0
        }
    }

    fn parse_method(self: &mut Self, method_buf: [u8; 4]) {
        let method: u32 = unsafe { std::mem::transmute(method_buf) };
        match method {
            GET_U32 => {
                self.method = HttpMethod::Get;
                self.state = HttpRequestParserState::ParsePath;
            }
            POST_U32 => {
                self.method = HttpMethod::Post;
                self.state = HttpRequestParserState::SkipSpace;
            }
            _ => self.method = HttpMethod::Unknown,
        }
    }

    fn feed(self: &mut Self, buf: &[u8], n: usize) {
        let mut buf_pos = 0;
        while buf_pos < buf.len() {
            match self.state {
                HttpRequestParserState::ParseMethod => {
                    if self.method_pos == 0 && n >= 4 {
                        buf_pos += 4;
                        self.parse_method(unsafe { std::mem::transmute(*(buf.as_ptr() as *const u32)) });
                    } else {
                        let diff = std::cmp::max(4 - self.method_pos, n);
                        for i in 0..diff {
                            self.method_buf[self.method_pos + i] = buf[i];
                        }
                        buf_pos += diff;
                        if self.method_pos == 4 {
                            self.parse_method(self.method_buf);
                        }
                    }
                }
                HttpRequestParserState::SkipSpace => {
                    buf_pos += 1;
                }
                HttpRequestParserState::ParsePath => {
                    let prev_pos = buf_pos;
                    while buf_pos < buf.len() && buf[buf_pos] != b'/' {
                        buf_pos += 1;
                    }
                    self.cur_path_component.extend(&buf[prev_pos..buf_pos]);
                    if buf[buf_pos] == b'/' {
                        self.path
                            .push(unsafe { String::from_utf8_unchecked(self.cur_path_component.clone()) });
                    }
                }
                HttpRequestParserState::SkipHeader => {
                    self.method_pos += 1;
                }
                HttpRequestParserState::ParseBody => {
                    self.buf.extend_from_slice(buf);
                }
            }
        }
    }

}

async fn conn_dispatcher(mut stream: TcpStream) {
    let mut temp_buffer = [0u8; 4096];
    let mut ctx = ParserContext::new();
    let (result, temp_buffer) = stream.read(temp_buffer).await;
    match result {
        Ok(0) => {
            println!("Connection closed");
            return;
        }
        Ok(_) => {
            ctx.buf.extend_from_slice(&temp_buffer[..result.unwrap()]);
        }
        Err(e) => {
            println!("Error: {}", e);
            return;
        }
    }
}

pub struct ZSet {
    value_to_score: HashMap<String, u32>,
    score_to_values: BTreeMap<u32, BTreeSet<String>>,
}

pub struct Storage {
    db: DBWithThreadMode<MultiThreaded>,
    zsets: RwLock<HashMap<String, ZSet>>,
}

async fn handle_query(key: &str, storage: Arc<Storage>) -> Result<Response<Body>, hyper::Error> {
    let db = &storage.db;
    let value = db.get(key.as_bytes()).unwrap();
    match value {
        Some(value) => Ok(Response::new(Body::from(value))),
        None => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap()),
    }
}

async fn handle_del(key: &str, storage: Arc<Storage>) -> Result<Response<Body>, hyper::Error> {
    let db = &storage.db;
    let kv = &storage.zsets;
    let mut kv = kv.write();
    db.delete(key).unwrap();
    kv.remove(key);
    Ok(Response::new(Body::empty()))
}

async fn handle_add(
    req: Request<Body>,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await?;
    let dto: dto::InsrtRequest = serde_json::from_slice(&data).unwrap();
    let db = &storage.db;
    db.put(&dto.key, &dto.value).unwrap();
    let kv = &storage.zsets;
    let mut kv = kv.write();
    if kv.contains_key(&dto.key) {
        kv.remove(&dto.key);
    }
    Ok(Response::new(Body::empty()))
}

async fn handle_zadd(
    key: &str,
    body: Body,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let data = hyper::body::to_bytes(body).await?;
    let dto: ScoreValue = serde_json::from_slice(&data).unwrap();
    let db = &storage.db;
    if db.get(key).unwrap().is_some() {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .unwrap());
    }
    let zsets = &storage.zsets;
    let mut zsets = zsets.write();
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
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let data = hyper::body::to_bytes(body).await?;
    let dto: ScoreRange = serde_json::from_slice(&data).unwrap();
    let zsets = &storage.zsets;
    let zsets = zsets.read();
    if let Some(zset) = zsets.get(key) {
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
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let zsets = &storage.zsets;
    let mut zsets = zsets.write();
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
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await?;
    let dto: Vec<dto::InsrtRequest> = serde_json::from_slice(&data).unwrap();
    let db = &storage.db;
    let mut write_batch = WriteBatch::default();
    for kv in dto {
        write_batch.put(kv.key, kv.value);
    }
    db.write(write_batch).unwrap();
    Ok(Response::new(Body::empty()))
}

async fn handle_list(
    req: Request<Body>,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await.unwrap();
    let dto: Vec<String> = serde_json::from_slice(&data).unwrap();
    let dto = &dto;
    let db = &storage.db;
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
    storage: Arc<Storage>,
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
    let db = DBWithThreadMode::<MultiThreaded>::open(&options, db_path).unwrap();
    let thread_count = std::env::var("THREAD_COUNT")
        .unwrap_or("16".to_string())
        .parse::<u32>()
        .unwrap();
    let storage = Arc::new(Storage {
        db,
        zsets: RwLock::new(HashMap::new()),
    });
    let mut threads = vec![];
    for _ in 0..thread_count {
        let storage = storage.clone();
        let thread = std::thread::spawn(|| {
            let mut rt = monoio::RuntimeBuilder::<monoio::IoUringDriver>::new()
                .enable_all()
                .with_entries(512)
                .build()
                .unwrap();
            println!("Running http server on 0.0.0.0:8080");
            rt.block_on(serve_http(([0, 0, 0, 0], 8080), hyper_handler, storage))
                .unwrap();
            println!("Http server stopped");
        });
        threads.push(thread);
    }
    for thread in threads {
        thread.join().unwrap();
    }
}
