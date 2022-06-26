mod dto;
#[cfg(feature = "glommio")]
mod glommio_hyper;
#[cfg(feature = "monoio")]
mod monoio_hyper;

use dto::{InsrtRequest, ScoreRange, ScoreValue};

use hyper::{Body, Method, Request, Response, StatusCode};
use lockfree_cuckoohash::LockFreeCuckooHash;
use parking_lot::RwLock;
use rocksdb::{DBWithThreadMode, MultiThreaded, Options};
use std::collections::HashMap;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

pub struct ZSet {
    value_to_score: HashMap<String, u32>,
    score_to_values: BTreeMap<u32, BTreeSet<String>>,
}

const SHARDS_COUNT: usize = 256;
const SHARD_MASK: u32 = 0xFF;

pub struct Storage {
    kv: Vec<LockFreeCuckooHash<String, String>>,
    zsets: Vec<RwLock<HashMap<String, ZSet>>>,
}

fn hash_key(key: &str) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    (hasher.finish() as u32 & SHARD_MASK) as usize
}

async fn handle_query(key: &str, storage: Arc<Storage>) -> Result<Response<Body>, hyper::Error> {
    let kv = &storage.kv[hash_key(key)];
    let guard = lockfree_cuckoohash::pin();
    match kv.get(key, &guard) {
        Some(value) => Ok(Response::new(Body::from(value.clone()))),
        None => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap()),
    }
}

async fn handle_del(key: &str, storage: Arc<Storage>) -> Result<Response<Body>, hyper::Error> {
    let shard = hash_key(key);
    {
        let kv = &storage.kv[shard];
        kv.remove(key);
    }
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
    let dto: dto::InsrtRequest = serde_json::from_slice(&data).unwrap();
    let shard = hash_key(&dto.key);
    let kv = &storage.kv[shard];
    let zsets = &storage.zsets[shard];
    let mut zsets = zsets.write();
    zsets.remove(&dto.key);
    kv.insert(dto.key, dto.value);
    Ok(Response::new(Body::empty()))
}

async fn handle_zadd(
    key: &str,
    body: Body,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let data = hyper::body::to_bytes(body).await?;
    let dto: ScoreValue = serde_json::from_slice(&data).unwrap();
    let shard = hash_key(key);
    {
        let kv = &storage.kv[shard];
        let guard = lockfree_cuckoohash::pin();
        if kv.get(key, &guard).is_some() {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .unwrap());
        }
    }
    let zsets = &storage.zsets[shard];
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
    let shard = hash_key(key);
    let zsets = &storage.zsets[shard];
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
        match serde_json::to_vec(&values) {
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

async fn handle_zrmv(
    key: &str,
    value: &str,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let shard = hash_key(key);
    let zsets = &storage.zsets[shard];
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
    for ins in dto {
        let shard = hash_key(&ins.key);
        let kv = &storage.kv[shard];
        kv.insert(ins.key, ins.value);
    }
    Ok(Response::new(Body::empty()))
}

async fn handle_list(
    req: Request<Body>,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await.unwrap();
    let dto: Vec<String> = serde_json::from_slice(&data).unwrap();
    let mut kvs: Vec<InsrtRequest> = vec![];
    for key in dto {
        let shard = hash_key(&key);
        let kv = &storage.kv[shard];
        let guard = lockfree_cuckoohash::pin();
        if let Some(value) = kv.get(&key, &guard) {
            kvs.push(InsrtRequest {
                key,
                value: value.clone(),
            });
        }
    }
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
    let db = DBWithThreadMode::<MultiThreaded>::open(&options, db_path).unwrap();
    let mut kv = HashMap::new();
    for (k, v) in db.iterator(rocksdb::IteratorMode::Start) {
        unsafe {
            let k = String::from_utf8_unchecked(k.to_vec());
            let v = String::from_utf8_unchecked(v.to_vec());
            kv.insert(k, v);
        }
    }
    let storage = Arc::new(Storage {
        kv: std::iter::repeat_with(|| LockFreeCuckooHash::new())
            .take(SHARDS_COUNT)
            .collect(),
        zsets: std::iter::repeat_with(|| RwLock::new(HashMap::new()))
            .take(SHARDS_COUNT)
            .collect(),
    });
    std::thread::spawn(|| {
        let guard = pprof::ProfilerGuard::new(100).unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            match guard.report().build() {
                Ok(report) => {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
                    let file = std::fs::File::create(format!("{}.svg", now.as_secs())).unwrap();
                    report.flamegraph(file).unwrap();
                }
                Err(e) => println!("{}", e),
            }
        }
    });

    #[cfg(feature = "tokio")]
    tokio_run(storage);

    #[cfg(feature = "glommio")]
    glommio_run(storage);

    #[cfg(feature = "monoio")]
    monoio_run(storage);
}
