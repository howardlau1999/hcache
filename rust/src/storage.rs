use crate::cluster::CachePeerClient;
use crate::dto::InsrtRequest;
#[cfg(feature = "memory")]
use lockfree_cuckoohash::{pin, LockFreeCuckooHash};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::{collections::hash_map::DefaultHasher, net::SocketAddr};
use tarpc::{client, context};
use tarpc::tokio_serde::formats::Bincode;
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

pub struct PeerClientPool {
    clients: Vec<Option<Arc<CachePeerClient>>>,
    me: u32,
}

impl Default for PeerClientPool {
    fn default() -> Self {
        Self {
            clients: Vec::new(),
            me: 0,
        }
    }
}

impl PeerClientPool {
    pub async fn new(peers: Vec<String>, me: u32) -> Self {
        let mut clients = Vec::new();
        for idx in 0..peers.len() {
            if idx == (me - 1) as usize {
                clients.push(None);
            } else {
                let hostport = peers[idx].clone() + ":58080";
                let addr = hostport.parse::<SocketAddr>().unwrap();
                let transport = tarpc::serde_transport::tcp::connect(addr, Bincode::default);
                let client =
                    CachePeerClient::new(client::Config::default(), transport.await.unwrap())
                        .spawn();
                clients.push(Some(Arc::new(client)));
            }
        }
        Self { clients, me }
    }

    pub fn count(&self) -> usize {
        self.clients.len()
    }

    pub fn get_client(&self, idx: usize) -> Arc<CachePeerClient> {
        self.clients[idx].as_ref().unwrap().clone()
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
        let pool = &self.cluster.read().await.pool;
        let client = pool.get_client(shard);
        let res = client.query(context::current(), key.to_string()).await;
        match res {
            Ok(value) => value,
            Err(_) => None,
        }
    }

    pub fn insert_kv(&self, key: &str, value: &str) -> Result<(), ()> {
        self.insert_kv_in_memory(key, value)
    }

    pub async fn insert_kv_remote(&self, key: &str, value: &str, shard: usize) -> Result<(), ()> {
        let pool = &self.cluster.read().await.pool;
        let client = pool.get_client(shard);
        let res = client.add(context::current(), key.to_string(), value.to_string()).await;
        match res {
            Ok(_) => Ok(()),
            Err(_) => Err(()),
        }
    }

    pub fn batch_insert_kv(&self, kvs: Vec<InsrtRequest>) -> Result<(), ()> {
        self.batch_insert_kv_in_memory(kvs)
    }

    pub fn list_keys(&self, keys: HashSet<String>) -> Vec<InsrtRequest> {
        self.list_keys_in_memory(keys)
    }

    pub fn remove_key(&self, key: &str) -> Result<(), ()> {
        self.remove_key_in_memory(key)
    }

    pub async fn remove_key_remote(&self, key: &str, shard: usize) -> Result<(), ()> {
        let pool = &self.cluster.read().await.pool;
        let client = pool.get_client(shard);
        let res = client.del(context::current(), key.to_string()).await;
        match res {
            Ok(_) => Ok(()),
            Err(_) => Err(()),
        }
    }

    pub async fn update_peers(&self, peers: Vec<String>, me: u32) {
        let mut cluster = self.cluster.write().await;
        self.me.store(me - 1, std::sync::atomic::Ordering::Relaxed);
        self.count.store(peers.len() as u64, std::sync::atomic::Ordering::Relaxed);
        cluster.pool = Arc::new(PeerClientPool::new(peers, me).await);
    }
}
