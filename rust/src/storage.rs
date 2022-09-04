use crate::cluster::CachePeerClient;
use crate::dto::{InsrtRequest, ScoreRange, ScoreValue};
#[cfg(feature = "memory")]
use lockfree_cuckoohash::{pin, LockFreeCuckooHash};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::Arc;
use std::{collections::hash_map::DefaultHasher, net::SocketAddr};
use tarpc::tokio_serde::formats::Bincode;
use tarpc::{client, context};
use tokio::sync::RwLock;

#[cfg(not(feature = "memory"))]
use rocksdb::WriteBatch;

use std::collections::{BTreeMap, HashSet};

pub fn hash_key<T>(key: T) -> u64
where
    T: Hash,
{
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

pub fn get_shard<T>(key: T, count: u64) -> usize
where
    T: Hash,
{
    (hash_key(key) % count) as usize
}

pub struct PeerClientQueue {
    pub addr: SocketAddr,
    pub clients: tokio::sync::RwLock<Vec<Arc<CachePeerClient>>>,
}

impl PeerClientQueue {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            clients: tokio::sync::RwLock::new(vec![]),
        }
    }

    pub async fn get_client(&self) -> Arc<CachePeerClient> {
        if self.clients.read().await.is_empty() {
            let mut clients = self.clients.write().await;
            let transport = tarpc::serde_transport::tcp::connect(self.addr, Bincode::default)
                .await
                .unwrap();

            let client = CachePeerClient::new(client::Config::default(), transport).spawn();
            clients.push(Arc::new(client));
        }
        self.clients.read().await[0].clone()
    }

    pub async fn put_client(&self, _: Arc<CachePeerClient>) {}
}

pub struct PeerClientPool {
    clients: Vec<PeerClientQueue>,
}

impl Default for PeerClientPool {
    fn default() -> Self {
        Self {
            clients: Vec::new(),
        }
    }
}

impl PeerClientPool {
    pub async fn new(peers: Vec<String>, me: u32) -> Self {
        let mut clients = Vec::new();
        for idx in 0..peers.len() {
            let hostport = peers[idx].clone() + ":58080";
            let addr = hostport.parse::<SocketAddr>().unwrap();
            let client = PeerClientQueue::new(addr);
            clients.push(client);
        }
        Self { clients }
    }

    pub async fn get_client(&self, idx: usize) -> Arc<CachePeerClient> {
        self.clients[idx].get_client().await
    }

    pub async fn put_client(&self, idx: usize, client: Arc<CachePeerClient>) {
        self.clients[idx].put_client(client).await;
    }
}

pub struct ZSet {
    pub value_to_score: LockFreeCuckooHash<String, u32>,
    pub score_to_values: parking_lot::RwLock<BTreeMap<u32, parking_lot::RwLock<HashSet<String>>>>,
}

pub struct ClusterInfo {
    pub pool: Arc<PeerClientPool>,
    pub peers: Vec<String>,
}

pub struct Storage {
    #[cfg(not(feature = "memory"))]
    pub db: DBWithThreadMode<MultiThreaded>,
    #[cfg(feature = "memory")]
    pub kv: LockFreeCuckooHash<String, String>,
    pub zsets: LockFreeCuckooHash<String, ZSet>,
    pub cluster: RwLock<ClusterInfo>,
    pub me: AtomicU32,
    pub count: AtomicU64,
    pub peer_updated: AtomicBool,
}

#[cfg(not(feature = "memory"))]
impl Storage {
    fn get_kv_in_db(&self, key: &str) -> Option<String> {
        self.db
            .get(key)
            .ok()
            .flatten()
            .map(|value| unsafe { String::from_utf8_unchecked(value) })
    }

    fn insert_kv_in_db(&self, key: &str, value: &str) -> Result<(), ()> {
        self.db.put(key, value).map_err(|_| ())
    }

    fn batch_insert_kv_in_db(&self, kvs: Vec<InsrtRequest>) -> Result<(), ()> {
        let db = &self.db;
        let mut write_batch = WriteBatch::default();
        for kv in kvs {
            {
                let shard = get_shard(&kv.key);
                let zsets = &self.zsets[shard];
                let mut zsets = zsets.write();
                zsets.remove(&kv.key);
            }
            write_batch.put(kv.key, kv.value);
        }
        db.write(write_batch).map_err(|_| ())
    }

    fn list_keys_in_db(&self, keys: HashSet<String>) -> Vec<InsrtRequest> {
        self.db
            .multi_get(&keys)
            .into_iter()
            .zip(keys.into_iter())
            .filter_map(|(res, key)| match res {
                Ok(value) => value.map(|value| InsrtRequest {
                    key,
                    value: unsafe { String::from_utf8_unchecked(value) },
                }),
                _ => None,
            })
            .collect()
    }

    fn remove_key_in_db(&self, key: &str) -> Result<(), ()> {
        self.db.delete(key).map_err(|_| ())
    }

    pub fn get_kv(&self, key: &str) -> Option<String> {
        self.get_kv_in_db(key)
    }

    pub fn insert_kv(&self, key: &str, value: &str) -> Result<(), ()> {
        self.insert_kv_in_db(key, value)
    }

    pub fn batch_insert_kv(&self, kvs: Vec<InsrtRequest>) -> Result<(), ()> {
        self.batch_insert_kv_in_db(kvs)
    }

    pub fn list_keys(&self, keys: HashSet<String>) -> Vec<InsrtRequest> {
        self.list_keys_in_db(keys)
    }

    pub fn remove_key(&self, key: &str) -> Result<(), ()> {
        self.remove_key_in_db(key)
    }
}

#[cfg(feature = "memory")]
impl Storage {
    fn get_kv_in_memory(&self, key: &str) -> Option<String> {
        let guard = pin();
        self.kv.get(key, &guard).cloned()
    }

    fn insert_kv_in_memory(&self, key: &str, value: &str) -> Result<(), ()> {
        self.kv.insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn batch_insert_kv_in_memory(&self, kvs: Vec<InsrtRequest>) -> Result<(), ()> {
        for kv in kvs {
            self.kv.insert(kv.key, kv.value);
        }
        Ok(())
    }

    fn list_keys_in_memory(&self, keys: HashSet<String>) -> Vec<InsrtRequest> {
        keys.into_iter()
            .filter_map(|key| {
                let guard = pin();
                self.kv.get(&key, &guard).map(|value| InsrtRequest {
                    key,
                    value: value.clone(),
                })
            })
            .collect()
    }

    fn remove_key_in_memory(&self, key: &str) -> Result<(), ()> {
        self.kv.remove(key);
        Ok(())
    }

    pub fn get_kv(&self, key: &str) -> Option<String> {
        self.get_kv_in_memory(key)
    }

    pub async fn get_kv_remote(&self, key: &str, shard: usize) -> Option<String> {
        let client = {
            let pool = &self.cluster.read().await.pool;
            pool.get_client(shard).await
        };
        let res = client.query(context::current(), key.to_string()).await;
        match res {
            Ok(value) => {
                self.cluster
                    .write()
                    .await
                    .pool
                    .put_client(shard, client)
                    .await;
                value
            }
            Err(_) => None,
        }
    }

    pub fn insert_kv(&self, key: &str, value: &str) -> Result<(), ()> {
        self.zsets.remove(key);
        self.insert_kv_in_memory(key, value)
    }

    pub async fn insert_kv_remote(&self, key: &str, value: &str, shard: usize) -> Result<(), ()> {
        let client = {
            let pool = &self.cluster.read().await.pool;
            pool.get_client(shard).await
        };
        let res = client
            .add(context::current(), key.to_string(), value.to_string())
            .await;
        match res {
            Ok(_) => {
                self.cluster
                    .write()
                    .await
                    .pool
                    .put_client(shard, client)
                    .await;
                Ok(())
            }
            Err(_) => Err(()),
        }
    }

    pub fn batch_insert_kv(&self, kvs: Vec<InsrtRequest>) -> Result<(), ()> {
        self.batch_insert_kv_in_memory(kvs)
    }

    pub async fn batch_insert_kv_remote(
        &self,
        kvs: Vec<InsrtRequest>,
        shard: usize,
    ) -> Result<(), ()> {
        let client = {
            let pool = &self.cluster.read().await.pool;
            pool.get_client(shard).await
        };
        let res = client.batch(context::current(), kvs).await;
        match res {
            Ok(_) => {
                self.cluster
                    .write()
                    .await
                    .pool
                    .put_client(shard, client)
                    .await;
                Ok(())
            }
            Err(_) => Err(()),
        }
    }

    pub fn list_keys(&self, keys: HashSet<String>) -> Vec<InsrtRequest> {
        self.list_keys_in_memory(keys)
    }

    pub async fn list_keys_remote(&self, keys: Vec<String>, shard: usize) -> Vec<InsrtRequest> {
        let client = {
            let pool = &self.cluster.read().await.pool;
            pool.get_client(shard).await
        };
        let res = client.list(context::current(), keys).await;
        match res {
            Ok(kvs) => {
                self.cluster
                    .write()
                    .await
                    .pool
                    .put_client(shard, client)
                    .await;
                kvs
            }
            Err(_) => vec![],
        }
    }

    pub fn remove_key(&self, key: &str) -> Result<(), ()> {
        self.remove_key_in_memory(key)
    }

    pub async fn remove_key_remote(&self, key: &str, shard: usize) -> Result<(), ()> {
        let client = {
            let pool = &self.cluster.read().await.pool;
            pool.get_client(shard).await
        };
        let res = client.del(context::current(), key.to_string()).await;
        match res {
            Ok(_) => {
                self.cluster
                    .write()
                    .await
                    .pool
                    .put_client(shard, client)
                    .await;
                Ok(())
            }
            Err(_) => Err(()),
        }
    }

    pub fn zadd(&self, key: &str, score_value: ScoreValue) {
        let guard = pin();
        let zset = self.zsets.get_or_insert(
            key.to_string(),
            ZSet {
                value_to_score: LockFreeCuckooHash::new(),
                score_to_values: parking_lot::RwLock::new(Default::default()),
            },
            &guard,
        );
        let value = score_value.value;
        let new_score = score_value.score;
        let old_score = zset.value_to_score.get(&value, &guard).copied();
        if let Some(score) = old_score {
            if score != new_score {
                {
                    // Remove from old score
                    let mut score_to_values = zset.score_to_values.write();

                    score_to_values.entry(score).and_modify(|values| {
                        values.write().remove(&value);
                    });
                    // Add to new score
                    score_to_values
                        .entry(new_score)
                        .or_insert_with(Default::default)
                        .write()
                        .insert(value.clone());
                }
                // Modify score
                zset.value_to_score.insert(value, new_score);
            }
        } else {
            zset.value_to_score.insert(value.clone(), new_score);
            zset.score_to_values
                .write()
                .entry(new_score)
                .or_insert_with(Default::default)
                .write()
                .insert(value);
        }
    }

    pub async fn zadd_remote(
        &self,
        key: &str,
        score_value: ScoreValue,
        shard: usize,
    ) -> Result<(), ()> {
        let client = {
            let pool = &self.cluster.read().await.pool;
            pool.get_client(shard).await
        };
        let res = client
            .zadd(context::current(), key.to_string(), score_value)
            .await;
        match res {
            Ok(_) => {
                self.cluster
                    .write()
                    .await
                    .pool
                    .put_client(shard, client)
                    .await;
                Ok(())
            }
            Err(_) => Err(()),
        }
    }

    pub fn zrange(&self, key: &str, score_range: ScoreRange) -> Option<Vec<ScoreValue>> {
        let guard = pin();
        let min_score = score_range.min_score;
        let max_score = score_range.max_score;
        self.zsets.get(key, &guard).map(|zset| {
            zset.score_to_values
                .read()
                .range(min_score..=max_score)
                .map(|(score, assoc_values)| {
                    let assoc_values = assoc_values.read().clone();
                    assoc_values.into_iter().map(|value| ScoreValue {
                        score: *score,
                        value,
                    })
                })
                .flatten()
                .collect()
        })
    }

    pub async fn zrange_remote(
        &self,
        key: &str,
        score_range: ScoreRange,
        shard: usize,
    ) -> Option<Vec<ScoreValue>> {
        let client = {
            let pool = &self.cluster.read().await.pool;
            pool.get_client(shard).await
        };
        let res = client
            .zrange(context::current(), key.to_string(), score_range)
            .await;
        match res {
            Ok(values) => {
                self.cluster
                    .write()
                    .await
                    .pool
                    .put_client(shard, client)
                    .await;
                values
            }
            Err(_) => None,
        }
    }

    pub fn zrmv(&self, key: &str, value: &str) {
        let guard = pin();
        if let Some(zset) = self.zsets.get(key, &guard) {
            let score = zset.value_to_score.remove_with_guard(value, &guard);
            if let Some(score) = score {
                zset.score_to_values
                    .write()
                    .entry(*score)
                    .and_modify(|values| {
                        values.write().remove(value);
                    });
            }
        }
    }

    pub async fn zrmv_remote(&self, key: &str, value: &str, shard: usize) -> Result<(), ()> {
        let client = {
            let pool = &self.cluster.read().await.pool;
            pool.get_client(shard).await
        };
        let res = client
            .zrmv(context::current(), key.to_string(), value.to_string())
            .await;
        match res {
            Ok(_) => {
                self.cluster
                    .write()
                    .await
                    .pool
                    .put_client(shard, client)
                    .await;
                Ok(())
            }
            Err(_) => Err(()),
        }
    }

    pub async fn update_peers(&self, peers: Vec<String>, me: u32) {
        let mut cluster = self.cluster.write().await;
        self.peer_updated
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.me.store(me - 1, std::sync::atomic::Ordering::Relaxed);
        self.count
            .store(peers.len() as u64, std::sync::atomic::Ordering::Relaxed);
        cluster.pool = Arc::new(PeerClientPool::new(peers, me).await);
    }
}
