mod dto;
#[cfg(feature = "glommio")]
mod glommio_hyper;
#[cfg(feature = "monoio")]
mod monoio_hyper;

use dto::{InsrtRequest, ScoreRange, ScoreValue};

use hyper::{Body, Method, Request, Response, StatusCode};
use parking_lot::RwLock;
use rocksdb::{DBWithThreadMode, MultiThreaded, Options, WriteBatch};
use std::collections::HashMap;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

const SHARD_COUNT: usize = 256;
const SHARD_MASK: usize = SHARD_COUNT - 1;

pub struct ZSet {
    value_to_score: HashMap<String, u32>,
    score_to_values: BTreeMap<u32, BTreeSet<String>>,
}

pub struct Storage {
    db: DBWithThreadMode<MultiThreaded>,
    zsets: Vec<RwLock<HashMap<String, RwLock<ZSet>>>>,
}

fn get_shard(key: &str) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish() as usize & SHARD_MASK
}

async fn handle_query(key: &str, storage: Arc<Storage>) -> Result<Response<Body>, hyper::Error> {
    let db = &storage.db;
    let value = db.get(key.as_bytes()).ok().flatten().map_or_else(
        || {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap())
        },
        |value| Ok(Response::new(Body::from(value))),
    );
    value
}

async fn handle_del(key: &str, storage: Arc<Storage>) -> Result<Response<Body>, hyper::Error> {
    let db = &storage.db;
    if let Err(e) = db.delete(key) {
        eprintln!("{:?}", e);
    }
    let shard = get_shard(key);
    let zsets = &storage.zsets[shard];
    let mut zsets = zsets.write();
    zsets.remove(key);
    Ok(Response::new(Body::empty()))
}

async fn handle_add(
    req: Request<Body>,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await?;
    let db = &storage.db;
    let dto: dto::InsrtRequest = serde_json::from_slice(&data).unwrap();
    {
        let shard = get_shard(&dto.key);
        let zsets = &storage.zsets[shard];
        let mut zsets = zsets.write();
        zsets.remove(&dto.key);
    }
    if let Err(_) = db.put(&dto.key, &dto.value) {
        Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .unwrap())
    } else {
        Ok(Response::new(Body::empty()))
    }
}

async fn handle_zadd(
    key: &str,
    body: Body,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let data = hyper::body::to_bytes(body).await?;
    let dto: ScoreValue = serde_json::from_slice(&data).unwrap();
    let db = &storage.db;
    if db.get(key).ok().flatten().is_some() {
        return Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .unwrap());
    }
    let shard = get_shard(key);
    let zsets = &storage.zsets[shard];
    let mut zsets = zsets.write();
    let zset = zsets.entry(key.to_string()).or_insert_with(|| RwLock::new(ZSet {
        value_to_score: HashMap::new(),
        score_to_values: BTreeMap::new(),
    }));
    let value = dto.value;
    let new_score = dto.score;
    let old_score = zset.read().value_to_score.get(&value).copied();
    if let Some(score) = old_score  {
        if score != new_score {
            // Remove from old score
            zset.write().score_to_values.entry(score).and_modify(|values| {
                values.remove(&value);
            });
            // Add to new score
            zset.write().score_to_values
                .entry(new_score)
                .or_insert_with(BTreeSet::new)
                .insert(value.clone());
            // Modify score
            zset.write().value_to_score.insert(value, new_score);
        }
    } else {
        zset.write().value_to_score.insert(value.clone(), new_score);
        zset.write().score_to_values
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
    match serde_json::from_slice::<ScoreRange>(&data) {
        Ok(dto) => {
            let shard = get_shard(key);
            let zsets = &storage.zsets[shard];
            let zsets = zsets.read();
            match zsets.get(key) {
                Some(zset) => {
                    let min_score = dto.min_score;
                    let max_score = dto.max_score;
                    let values: Vec<_> = zset.read()
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
                    match serde_json::to_vec(&values) {
                        Ok(json_bytes) => Ok(Response::new(Body::from(json_bytes))),
                        Err(_) => Ok(Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(Body::empty())
                            .unwrap()),
                    }
                }
                None => Ok(Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap()),
            }
        }
        Err(_) => Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .unwrap()),
    }
}

async fn handle_zrmv(
    key: &str,
    value: &str,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let shard = get_shard(key);
    let zsets = &storage.zsets[shard];
    let mut zsets = zsets.write();
    if let Some(zset) = zsets.get_mut(key) {
        let score = zset.write().value_to_score.remove(value);
        if let Some(score) = score {
            zset.write().score_to_values.entry(score).and_modify(|values| {
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

    match serde_json::from_slice::<Vec<dto::InsrtRequest>>(&data) {
        Ok(dto) => {
            let db = &storage.db;
            let mut write_batch = WriteBatch::default();
            for kv in dto {
                {
                    let shard = get_shard(&kv.key);
                    let zsets = &storage.zsets[shard];
                    let mut zsets = zsets.write();
                    zsets.remove(&kv.key);
                }
                write_batch.put(kv.key, kv.value);
            }
            match db.write(write_batch) {
                Ok(_) => Ok(Response::new(Body::empty())),
                Err(_) => Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::empty())
                    .unwrap()),
            }
        }
        Err(_) => Ok(Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::empty())
            .unwrap()),
    }
}

async fn handle_list(
    req: Request<Body>,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await.unwrap();
    let dto: Vec<String> = serde_json::from_slice(&data).unwrap();
    let db = &storage.db;
    let kvs: Vec<InsrtRequest> = db
        .multi_get(&dto)
        .into_iter()
        .zip(dto.into_iter())
        .filter_map(|(res, key)| match res {
            Ok(value) => value.map(|value| InsrtRequest {
                key,
                value: unsafe { String::from_utf8_unchecked(value) },
            }),
            _ => None,
        })
        .collect();
    if kvs.len() != 0 {
        match serde_json::to_vec(&kvs) {
            Ok(json_bytes) => Ok(Response::new(Body::from(json_bytes))),
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
            _ => Ok(Response::new(Body::from("ok"))),
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
                .body(Body::empty())
                .unwrap()),
        };
    }
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from("404 not found"))
        .unwrap())
}

#[cfg(feature = "tokio")]
fn tokio_run(storage: Arc<Storage>) {
    use hyper::{
        service::{make_service_fn, service_fn},
        Server,
    };
    use std::net::SocketAddr;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
        let make_svc = make_service_fn(|_| {
            let storage = storage.clone();
            async move {
                Ok::<_, hyper::Error>(service_fn(move |req| hyper_handler(req, storage.clone())))
            }
        });
        let server = Server::bind(&addr).serve(make_svc);
        server.await
    })
    .unwrap();
}

#[cfg(feature = "glommio")]
fn glommio_run(storage: Arc<Storage>) {
    use glommio::{CpuSet, LocalExecutorPoolBuilder, PoolPlacement};
    LocalExecutorPoolBuilder::new(PoolPlacement::MaxSpread(
        num_cpus::get(),
        CpuSet::online().ok(),
    ))
    .on_all_shards(|| async move {
        let id = glommio::executor().id();
        println!("Starting executor {}", id);
        glommio_hyper::serve_http(([0, 0, 0, 0], 8080), hyper_handler, 1024, storage)
            .await
            .unwrap();
    })
    .unwrap()
    .join_all();
}

#[cfg(feature = "monoio")]
fn monoio_run(storage: Arc<Storage>) {
    let mut threads = vec![];
    for _ in 0..num_cpus::get() {
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

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let db_path = Path::new(&args[1]);
    let mut options = Options::default();
    options.create_if_missing(true);
    options.set_allow_mmap_reads(true);
    options.set_allow_mmap_writes(true);
    options.set_unordered_write(true);
    options.set_use_adaptive_mutex(true);
    let db = DBWithThreadMode::<MultiThreaded>::open(&options, db_path).unwrap();
    let storage = Arc::new(Storage {
        db,
        zsets: std::iter::repeat_with(||  RwLock::new(HashMap::new())).take(SHARD_COUNT).collect(),
    });

    #[cfg(feature = "tokio")]
    tokio_run(storage);

    #[cfg(feature = "glommio")]
    glommio_run(storage);

    #[cfg(feature = "monoio")]
    monoio_run(storage);
}
