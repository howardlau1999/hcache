use hyper::{client::HttpConnector, Body, Client, Method, Request, StatusCode};
mod dto;
use dto::{InsrtRequest};
use lazy_static::lazy_static;
use tokio::time::Instant;

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
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Benchmark add
    let n: usize =
        std::env::var("N").map_or(10000, |value| value.parse::<usize>().unwrap_or(10000));

    // Benchmark batch
    let kvs: Vec<InsrtRequest> = (0..n)
        .map(|i| InsrtRequest {
            key: format!("key{}", i).repeat(16),
            value: String::from_iter(std::iter::repeat('a').take(1024)),
        })
        .collect();
    let tik = Instant::now();
    let mut handles = vec![];
    for chunk in kvs.chunks(1000) {
        handles.push(tokio::spawn(batch(chunk.to_vec())));
    }
    for handle in handles {
        handle.await.unwrap().unwrap();
    }
    let tok = Instant::now();
    let batch_duration = tok.duration_since(tik);
    println!("N: {} batch_duration: {:?}", n, batch_duration);

    Ok(())
}
