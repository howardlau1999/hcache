mod cluster;
mod dto;
mod storage;
use storage::{
    get_shard, ClusterInfo, Storage, LOAD_STATE_INIT, LOAD_STATE_LOADED, LOAD_STATE_LOADING,
};

#[cfg(feature = "memory")]
use lockfree_cuckoohash::LockFreeCuckooHash;

#[cfg(feature = "glommio")]
mod glommio_hyper;
#[cfg(feature = "monoio")]
mod monoio_hyper;

#[cfg(feature = "monoio_parser")]
mod http_parser;
#[cfg(feature = "monoio_parser")]
mod monoio_parser;
#[cfg(feature = "monoio_parser")]
use monoio_parser::monoio_parser_run;

use dto::{ScoreRange, ScoreValue};

use hyper::{Body, Method, Request, Response, StatusCode};

#[cfg(not(feature = "memory"))]
use rocksdb::WriteBatch;

use rocksdb::{
    DBWithThreadMode, FlushOptions, IteratorMode, MultiThreaded, Options, SingleThreaded,
    WriteOptions,
};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

async fn handle_query(key: &str, storage: Arc<Storage>) -> Result<Response<Body>, hyper::Error> {
    let shard = get_shard(key, storage.count.load(std::sync::atomic::Ordering::SeqCst)) as u32;

    let me = storage.me.load(std::sync::atomic::Ordering::SeqCst);
    let value = if shard == me {
        storage.get_kv(key)
    } else {
        let res = storage.get_kv_remote(key, shard as usize).await;
        match res {
            Ok(v) => v,
            Err(_) => {
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .unwrap())
            }
        }
    };
    let value = value.map_or_else(
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
    let shard = get_shard(key, storage.count.load(std::sync::atomic::Ordering::SeqCst)) as u32;
    let me = storage.me.load(std::sync::atomic::Ordering::SeqCst);
    if shard == me {
        storage.remove_key(key).unwrap();
        storage.zsets.remove(key);
        Ok(Response::new(Body::empty()))
    } else {
        match storage.remove_key_remote(key, shard as usize).await {
            Ok(_) => Ok(Response::new(Body::empty())),
            Err(_) => Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()),
        }
    }
}

async fn handle_add(
    req: Request<Body>,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await?;
    let kv: dto::InsrtRequest = serde_json::from_slice(&data).unwrap();
    let shard = get_shard(
        &kv.key,
        storage.count.load(std::sync::atomic::Ordering::SeqCst),
    ) as u32;
    let me = storage.me.load(std::sync::atomic::Ordering::SeqCst);
    if shard == me {
        storage.insert_kv(&kv.key, &kv.value).unwrap();
        Ok(Response::new(Body::empty()))
    } else {
        match storage
            .insert_kv_remote(&kv.key, &kv.value, shard as usize)
            .await
        {
            Ok(_) => Ok(Response::new(Body::empty())),
            Err(_) => Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()),
        }
    }
}

async fn handle_zadd(
    key: &str,
    body: Body,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let data = hyper::body::to_bytes(body).await?;
    let dto: ScoreValue = serde_json::from_slice(&data).unwrap();
    let shard = get_shard(key, storage.count.load(std::sync::atomic::Ordering::SeqCst)) as u32;
    let me = storage.me.load(std::sync::atomic::Ordering::SeqCst);
    if shard == me {
        storage.zadd(key, dto);
        Ok(Response::new(Body::empty()))
    } else {
        match storage.zadd_remote(key, dto, shard as usize).await {
            Ok(_) => Ok(Response::new(Body::empty())),
            Err(_) => Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()),
        }
    }
}

async fn handle_zrange(
    key: &str,
    body: Body,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let data = hyper::body::to_bytes(body).await?;
    let dto = serde_json::from_slice::<ScoreRange>(&data).unwrap();
    let shard = get_shard(key, storage.count.load(std::sync::atomic::Ordering::SeqCst)) as u32;
    let me = storage.me.load(std::sync::atomic::Ordering::SeqCst);
    let value = if shard == me {
        storage.zrange(key, dto)
    } else {
        let value = storage.zrange_remote(key, dto, shard as usize).await;
        match value {
            Ok(v) => v,
            Err(_) => {
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .unwrap())
            }
        }
    };
    value.map_or_else(
        || {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap())
        },
        |value| {
            Ok(Response::new(Body::from(
                serde_json::to_string(&value).unwrap(),
            )))
        },
    )
}

async fn handle_zrmv(
    key: &str,
    value: &str,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let shard = get_shard(key, storage.count.load(std::sync::atomic::Ordering::SeqCst)) as u32;
    let me = storage.me.load(std::sync::atomic::Ordering::SeqCst);
    if shard == me {
        storage.zrmv(key, value);
        Ok(Response::new(Body::empty()))
    } else {
        match storage.zrmv_remote(key, value, shard as usize).await {
            Ok(_) => Ok(Response::new(Body::empty())),
            Err(_) => Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()),
        }
    }
}

async fn handle_batch(
    req: Request<Body>,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await?;

    let kvs = serde_json::from_slice::<Vec<dto::InsrtRequest>>(&data).unwrap();
    let peers_count = storage.count.load(std::sync::atomic::Ordering::SeqCst);
    let sharded = HashMap::<u32, Vec<dto::InsrtRequest>>::new();
    let mut sharded = kvs.into_iter().fold(sharded, |mut acc, kv| {
        let shard = get_shard(&kv.key, peers_count) as u32;
        acc.entry(shard).or_insert_with(Vec::new).push(kv);
        acc
    });
    let me = storage.me.load(std::sync::atomic::Ordering::SeqCst);
    let mut futures = Vec::new();
    let my_keys = sharded.remove(&me).unwrap_or_default();
    for (shard, kvs) in sharded {
        let me = storage.me.load(std::sync::atomic::Ordering::SeqCst);
        let storage = storage.clone();
        if shard != me {
            futures.push(tokio::task::spawn_local(async move {
                storage.batch_insert_kv_remote(kvs, shard as usize).await
            }));
        }
    }
    storage.batch_insert_kv(my_keys).unwrap();
    for f in futures {
        f.await;
    }
    Ok(Response::new(Body::empty()))
}

async fn handle_list(
    req: Request<Body>,
    storage: Arc<Storage>,
) -> Result<Response<Body>, hyper::Error> {
    let body = req.into_body();
    let data = hyper::body::to_bytes(body).await?;
    let keys = serde_json::from_slice::<Vec<String>>(&data).unwrap();
    let peers_count = storage.count.load(std::sync::atomic::Ordering::SeqCst);
    let sharded = HashMap::<u32, HashSet<String>>::new();
    let mut sharded = keys.into_iter().fold(sharded, |mut acc, key| {
        let shard = get_shard(&key, peers_count) as u32;
        acc.entry(shard).or_insert_with(HashSet::new).insert(key);
        acc
    });
    let me = storage.me.load(std::sync::atomic::Ordering::SeqCst);
    let mut futures = Vec::new();
    let my_keys = sharded.remove(&me).unwrap_or_default();
    for (shard, keys) in sharded {
        let me = storage.me.load(std::sync::atomic::Ordering::SeqCst);
        let storage = storage.clone();
        if shard != me {
            futures.push(tokio::task::spawn_local(async move {
                storage
                    .list_keys_remote(keys.into_iter().collect(), shard as usize)
                    .await
            }));
        }
    }
    let mut result = storage.list_keys(my_keys);
    for handle in futures {
        if let Ok(Ok(mut keys)) = handle.await {
            result.append(&mut keys);
        }
    }
    if result.is_empty() {
        Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap())
    } else {
        Ok(Response::new(Body::from(
            serde_json::to_string(&result).unwrap(),
        )))
    }
}

async fn handle_updatecluster(body: Body) -> Result<Response<Body>, hyper::Error> {
    let data = hyper::body::to_bytes(body).await?;
    if let Ok(mut cluster_json) = tokio::fs::File::create("/data/cluster.json").await {
        cluster_json.write_all(&data).await.unwrap();
        Ok(Response::new(Body::from("ok")))
    } else {
        Ok(Response::new(Body::from("fail")))
    }
}

async fn try_load_peers(storage: Arc<Storage>) {
    let cluster_info_json = std::fs::File::open("/data/cluster.json");
    if let Ok(mut cluster_info_json) = cluster_info_json {
        let mut data = String::new();
        if let Ok(_) = cluster_info_json.read_to_string(&mut data) {
            if let Ok(cluster_info) = serde_json::from_str::<dto::UpdateCluster>(data.as_str()) {
                if let Ok(_) = storage.peer_updated.compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                ) {
                    storage
                        .update_peers(cluster_info.hosts, cluster_info.index)
                        .await;
                    println!("Peers updated");
                }
            }
        }
    }
}

async fn handle_init(storage: Arc<Storage>) -> Result<Response<Body>, hyper::Error> {
    try_load_peers(storage.clone()).await;
    if let Ok(_) = storage.load_state.compare_exchange(
        LOAD_STATE_INIT,
        LOAD_STATE_LOADING,
        Ordering::SeqCst,
        Ordering::SeqCst,
    ) {
        std::thread::spawn(|| {
            if let Ok(init_paths) = std::env::var("INIT_DIRS") {
                let init_paths = init_paths.split(',');
                let ssts = init_paths
                    .filter_map(|init_path| {
                        let init_path = init_path.to_string().clone();
                        let options = options.clone();
                        match DBWithThreadMode::<MultiThreaded>::open(&options, &init_path) {
                            Ok(db) => Some(std::thread::spawn(move || {
                                let mut iter = db.iterator(IteratorMode::Start);
                                let mut sst_writer = rocksdb::SstFileWriter::create(&options);
                                let sst_path = Path::new(init_path.as_str())
                                    .join("bulkload.sst")
                                    .to_path_buf();
                                sst_writer.open(&sst_path).unwrap();
                                while let Some((key, value)) = iter.next() {
                                    sst_writer.put(key, value).unwrap();
                                }
                                sst_writer.finish().unwrap();
                                sst_path
                            })),
                            Err(_) => None,
                        }
                    })
                    .map(|h| h.join().unwrap())
                    .collect();

                println!("{:?}", ssts);
                let mut ingest_opt = rocksdb::IngestExternalFileOptions::default();
                ingest_opt.set_snapshot_consistency(false);
                ingest_opt.set_move_files(true);
                ingest_opt.set_allow_global_seqno(true);
                storage.db.ingest_external_file_opts(&ingest_opt, ssts);
            }
            storage
                .load_state
                .store(LOAD_STATE_LOADED, Ordering::SeqCst);
        });
    }
    Ok(Response::new(Body::from("ok")))
}

#[cfg(not(feature = "monoio_parser"))]
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
            "init" => handle_init(storage).await,
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
            "updateCluster" => handle_updatecluster(req.into_body()).await,
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

#[cfg(feature = "tokio_local")]
fn tokio_local_run(storage: Arc<Storage>) {
    use core_affinity::{get_core_ids, set_for_current};
    use hyper::{server::conn::Http, service::service_fn};
    use std::net::SocketAddr;
    let core_ids = get_core_ids().unwrap();
    let mut worker_threads = vec![];
    for core_id in core_ids {
        let storage = storage.clone();
        let worker = std::thread::spawn(move || {
            println!("Starting worker {}", core_id.id);
            set_for_current(core_id);
            let local_rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            local_rt.block_on(async move {
                let local = tokio::task::LocalSet::new();
                local
                    .run_until(async move {
                        let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
                        let socket = tokio::net::TcpSocket::new_v4().unwrap();
                        socket.set_reuseport(true).unwrap();
                        socket.bind(addr).unwrap();
                        let listener = socket.listen(1024).unwrap();
                        while let Ok((conn, _addr)) = listener.accept().await {
                            let storage = storage.clone();
                            let http_conn = Http::new().serve_connection(
                                conn,
                                service_fn(move |req| hyper_handler(req, storage.clone())),
                            );
                            tokio::task::spawn_local(async move {
                                match http_conn.await {
                                    Ok(_) => {}
                                    Err(e) => {
                                        if !e.is_incomplete_message() {
                                            eprintln!("Error: {}", e);
                                        }
                                    }
                                }
                            });
                        }
                    })
                    .await;
            })
        });

        worker_threads.push(worker);
    }
    let rpc_worker = std::thread::spawn(move || {
        println!("Starting RPC worker");
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let f = tokio::spawn(cluster::cluster_server(storage.clone()));
            try_load_peers(storage).await;
            f.await.unwrap();
        })
    });
    worker_threads.push(rpc_worker);
    for worker in worker_threads {
        worker.join().unwrap();
    }
}

#[cfg(feature = "tokio_uring")]
fn tokio_uring_run(storage: Arc<Storage>) {
    use hyper::{server::conn::Http, service::service_fn};
    use std::net::SocketAddr;
    tokio_uring::start(async {
        let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
        let listener = tokio_uring::net::TcpListener::bind(addr).unwrap();
        while let Ok((conn, _addr)) = listener.accept().await {
            let storage = storage.clone();
            let http_conn = Http::new().serve_connection(
                conn,
                service_fn(move |req| hyper_handler(req, storage.clone())),
            );
            tokio::task::spawn_local(async move {
                match http_conn.await {
                    Ok(_) => {}
                    Err(e) => {
                        if !e.is_incomplete_message() {
                            eprintln!("Error: {}", e);
                        }
                    }
                }
            });
        }
    });
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
    let core_ids = core_affinity::get_core_ids().unwrap();
    let mut threads = vec![];
    for core_id in core_ids {
        let storage = storage.clone();
        let thread = std::thread::spawn(move || {
            core_affinity::set_for_current(core_id);
            let mut rt = monoio::RuntimeBuilder::<monoio::IoUringDriver>::new()
                .enable_all()
                .with_entries(512)
                .build()
                .unwrap();
            println!("Running http server on 0.0.0.0:8080");
            rt.block_on(monoio_hyper::serve_http(
                ([0, 0, 0, 0], 8080),
                hyper_handler,
                storage,
            ))
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
    #[cfg(feature = "memory")]
    let kv = LockFreeCuckooHash::new();

    #[cfg(not(feature = "memory"))]
    let db_path = Path::new(std::env::var("INIT_DIR").unwrap());
    #[cfg(not(feature = "memory"))]
    let db = DBWithThreadMode::<MultiThreaded>::open(&options, db_path).unwrap();

    let mut options = Options::default();
    options.create_if_missing(true);
    options.increase_parallelism(4);
    options.set_allow_mmap_reads(true);
    options.set_allow_mmap_writes(true);
    options.set_unordered_write(true);
    options.set_use_adaptive_mutex(true);
    let db = Arc::new(DBWithThreadMode::<MultiThreaded>::open(&options, "/data/kv").unwrap());
    let zset_db =
        Arc::new(DBWithThreadMode::<MultiThreaded>::open(&options, "/data/zset").unwrap());
    let mut write_options = WriteOptions::default();
    write_options.disable_wal(true);
    {
        let db = db.clone();
        let zset_db = zset_db.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            db.flush();
            zset_db.flush();
        });
    }
    let storage = Arc::new(Storage {
        #[cfg(not(feature = "memory"))]
        db,
        #[cfg(feature = "memory")]
        kv,
        zsets: LockFreeCuckooHash::new(),
        cluster: tokio::sync::RwLock::new(ClusterInfo {
            pool: Default::default(),
            peers: vec![],
        }),
        me: Default::default(),
        count: Default::default(),
        peer_updated: Default::default(),
        load_state: Default::default(),
        db,
        zset_db,
        write_options,
    });

    storage.load_from_disk();

    #[cfg(feature = "tokio_uring")]
    tokio_uring_run(storage);

    #[cfg(feature = "tokio_local")]
    tokio_local_run(storage);

    #[cfg(feature = "tokio")]
    tokio_run(storage);

    #[cfg(feature = "glommio")]
    glommio_run(storage);

    #[cfg(feature = "monoio")]
    monoio_run(storage);

    #[cfg(feature = "monoio_parser")]
    monoio_parser_run(storage);
}
