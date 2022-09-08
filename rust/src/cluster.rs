use futures::{
    future::{self, Ready},
    prelude::*,
};
use std::{net::SocketAddr, sync::Arc};
use tarpc::{
    context,
    server::{self, Channel},
    tokio_serde::formats::Bincode,
};

use crate::{
    dto::{InsrtRequest, ScoreRange, ScoreValue},
    storage::Storage,
};
// This is the service definition. It looks a lot like a trait definition.
// It defines one RPC, hello, which takes one arg, name, and returns a String.
#[tarpc::service]
pub trait CachePeer {
    /// Returns a greeting for name.
    async fn query(key: String) -> Option<String>;
    async fn add(key: String, value: String) -> ();
    async fn del(key: String) -> ();
    async fn list(keys: Vec<String>) -> Vec<InsrtRequest>;
    async fn batch(kvs: Vec<InsrtRequest>) -> ();
    async fn zadd(key: String, score_value: ScoreValue) -> ();
    async fn zrange(key: String, score_range: ScoreRange) -> Option<Vec<ScoreValue>>;
    async fn zrmv(key: String, value: String) -> ();
}

#[derive(Clone)]
pub struct CachePeerServer {
    storage: Arc<Storage>,
    addr: SocketAddr,
}

impl CachePeer for CachePeerServer {
    // Each defined rpc generates two items in the trait, a fn that serves the RPC, and
    // an associated type representing the future output by the fn.
    type QueryFut = Ready<Option<String>>;
    type AddFut = Ready<()>;
    type DelFut = Ready<()>;
    type ListFut = Ready<Vec<InsrtRequest>>;
    type BatchFut = Ready<()>;
    type ZaddFut = Ready<()>;
    type ZrangeFut = Ready<Option<Vec<ScoreValue>>>;
    type ZrmvFut = Ready<()>;

    fn query(self, _: context::Context, key: String) -> Self::QueryFut {
        future::ready(self.storage.get_kv(key.as_str()))
    }

    fn add(self, _: tarpc::context::Context, key: String, value: String) -> Self::AddFut {
        self.storage
            .insert_kv(key.as_str(), value.as_str())
            .unwrap();
        future::ready(())
    }

    fn del(self, _: tarpc::context::Context, key: String) -> Self::DelFut {
        self.storage.remove_key(key.as_str()).unwrap();
        self.storage.zsets.remove(key.as_str());
        future::ready(())
    }

    fn list(self, _: tarpc::context::Context, keys: Vec<String>) -> Self::ListFut {
        let mut result = Vec::new();
        for key in keys {
            if let Some(value) = self.storage.get_kv(key.as_str()) {
                result.push(InsrtRequest { key, value });
            }
        }
        future::ready(result)
    }

    fn batch(self, _: tarpc::context::Context, kvs: Vec<InsrtRequest>) -> Self::BatchFut {
        for kv in kvs {
            self.storage
                .insert_kv(kv.key.as_str(), kv.value.as_str())
                .unwrap();
        }
        future::ready(())
    }

    fn zadd(
        self,
        _: tarpc::context::Context,
        key: String,
        score_value: ScoreValue,
    ) -> Self::ZaddFut {
        future::ready(self.storage.zadd_memory(key.as_str(), score_value))
    }

    fn zrange(
        self,
        _: tarpc::context::Context,
        key: String,
        score_range: ScoreRange,
    ) -> Self::ZrangeFut {
        future::ready(self.storage.zrange(key.as_str(), score_range))
    }

    fn zrmv(self, _: tarpc::context::Context, key: String, value: String) -> Self::ZrmvFut {
        future::ready(self.storage.zrmv(key.as_str(), value.as_str()))
    }
}

pub async fn cluster_server(storage: Arc<Storage>) {
    let addr = "0.0.0.0:58080".parse::<SocketAddr>().unwrap();
    let mut listener = tarpc::serde_transport::tcp::listen(addr, Bincode::default)
        .await
        .unwrap();
    listener.config_mut().max_frame_length(usize::MAX);
    listener
        // Ignore accept errors.
        .filter_map(|r| future::ready(r.ok()))
        .map(server::BaseChannel::with_defaults)
        // serve is generated by the service attribute. It takes as input any type implementing
        // the generated World trait.
        .map(|channel| {
            let server = CachePeerServer {
                addr: channel.transport().peer_addr().unwrap(),
                storage: storage.clone(),
            };
            channel.execute(server.serve())
        })
        // Max 10 channels.
        .buffer_unordered(1000)
        .for_each(|_| async {})
        .await;
}
