use hyper::{client::HttpConnector, Body, Client, Method, Request, StatusCode};
mod dto;
use dto::InsrtRequest;
use lazy_static::lazy_static;
use tokio::{sync::Semaphore, io::AsyncWriteExt};

lazy_static! {
    pub static ref HOST: String = {
        String::from(
            std::env::var("HCACHE_HOST").unwrap_or_else(|_| "http://localhost:8080".to_string()),
        )
    };
    pub static ref CLIENT: Client<HttpConnector> = {
        let client = Client::new();
        client
    };
    pub static ref ADD_ENDPOINT: String = format!("{}/add", *HOST);
    pub static ref BATCH_ENDPOINT: String = format!("{}/batch", *HOST);
    pub static ref LIST_ENDPOINT: String = format!("{}/list", *HOST);
}

async fn batch(kvs: Vec<InsrtRequest>) -> Result<(), ()> {
    let client = &*CLIENT;
    let req = Request::builder()
        .method(Method::POST)
        .uri(&*BATCH_ENDPOINT)
        .body(Body::from(serde_json::to_string(&kvs).unwrap()))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    match resp.status() {
        StatusCode::OK => Ok(()),
        _ => Err(()),
    }
}

#[tokio::main]
async fn main() -> ! {
    // Benchmark add
    let n: usize = std::env::var("N").map_or(100, |value| value.parse::<usize>().unwrap_or(100));
    let sema = std::sync::Arc::new(Semaphore::new(n));

    let chunk: Vec<_> = (0..1000)
        .map(|i| InsrtRequest {
            key: format!("key{}", i).repeat(16),
            value: String::from_iter(std::iter::repeat('a').take(1024)),
        })
        .collect();
    let mut counter = 0;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<tokio::time::Duration>();
    tokio::spawn(async move {
        let mut opts = tokio::fs::OpenOptions::new();
        opts.create(true).write(true);
        let mut data = opts.open("req.txt").await.unwrap();
        while let Some(d) = rx.recv().await {
            data.write_all(format!("{}\n", d.as_nanos()).as_bytes()).await.unwrap();
        }
    });
    let mut prev = tokio::time::Instant::now();
    loop {
        sema.acquire().await.unwrap().forget();
        let sema = sema.clone();
        counter += 1;
        let chunk = chunk.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let ins = tokio::time::Instant::now();
            let _ = batch(chunk).await;
            sema.add_permits(1);
            tx.send(ins.elapsed()).unwrap();
        });
        if counter % 1000 == 0 {
            println!("{} {:?}", counter, prev.elapsed());
            prev = tokio::time::Instant::now();
        }
    }
}
