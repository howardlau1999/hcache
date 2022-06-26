use std::fmt::Display;

use hyper::{Body, Client, Method, Request, StatusCode, client::HttpConnector};
mod dto;
use dto::{InsrtRequest, ScoreRange, ScoreValue};
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
    pub static ref ADD_ENDPOINT: String = {
        format!("{}/add", *HOST)
    };
    pub static ref BATCH_ENDPOINT: String = {
        format!("{}/batch", *HOST)
    };
    pub static ref LIST_ENDPOINT: String = {
        format!("{}/list", *HOST)
    };
}

async fn query(key: String) -> Option<String> {
    let client = &*CLIENT;
    let resp = client
        .get(format!("{}/query/{}", *HOST, key).parse().unwrap())
        .await
        .unwrap();
    match resp.status() {
        StatusCode::OK => {
            let value = unsafe {
                String::from_utf8_unchecked(
                    hyper::body::to_bytes(resp.into_body())
                        .await
                        .unwrap()
                        .to_vec(),
                )
            };
            Some(value)
        }
        _ => None,
    }
}

async fn del(key: String) -> Result<(), ()> {
    let client = &*CLIENT;
    let resp = client
        .get(format!("{}/del/{}", *HOST, key).parse().unwrap())
        .await
        .unwrap();
    match resp.status() {
        StatusCode::OK => Ok(()),
        _ => Err(()),
    }
}

async fn add(key: String, value: String) -> Result<(), ()> {
    let client = &*CLIENT;
    let req = InsrtRequest { key, value };
    let req = Request::builder()
        .method(Method::POST)
        .uri(&*ADD_ENDPOINT)
        .body(Body::from(serde_json::to_string(&req).unwrap()))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    match resp.status() {
        StatusCode::OK => Ok(()),
        _ => Err(()),
    }
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

async fn list(keys: Vec<String>) -> Result<Vec<InsrtRequest>, ()> {
    let client = &*CLIENT;
    let req = Request::builder()
        .method(Method::POST)
        .uri(&*LIST_ENDPOINT)
        .body(Body::from(serde_json::to_string(&keys).unwrap()))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    match resp.status() {
        StatusCode::OK => {
            let value = unsafe {
                String::from_utf8_unchecked(
                    hyper::body::to_bytes(resp.into_body())
                        .await
                        .unwrap()
                        .to_vec(),
                )
            };
            Ok(serde_json::from_str(&value).unwrap())
        }
        _ => Err(()),
    }
}

async fn zadd(key: String, score: u32, value: String) -> Result<(), ()> {
    let client = &*CLIENT;
    let req = ScoreValue { score, value };
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("{}/zadd/{}", *HOST, key))
        .body(Body::from(serde_json::to_string(&req).unwrap()))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    match resp.status() {
        StatusCode::OK => Ok(()),
        _ => Err(()),
    }
}

async fn zrange(key: String, min_score: u32, max_score: u32) -> Result<Vec<ScoreValue>, ()> {
    let client = &*CLIENT;
    let req = ScoreRange {
        min_score,
        max_score,
    };
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("{}/zrange/{}", *HOST, key))
        .body(Body::from(serde_json::to_string(&req).unwrap()))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    match resp.status() {
        StatusCode::OK => {
            let value = unsafe {
                String::from_utf8_unchecked(
                    hyper::body::to_bytes(resp.into_body())
                        .await
                        .unwrap()
                        .to_vec(),
                )
            };
            Ok(serde_json::from_str(&value).unwrap())
        }
        _ => Err(()),
    }
}

async fn zrmv(key: String, value: String) -> Result<(), ()> {
    let client = &*CLIENT;
    let resp = client
        .get(format!("{}/zrmv/{}/{}", *HOST, key, value).parse().unwrap())
        .await
        .unwrap();
    match resp.status() {
        StatusCode::OK => Ok(()),
        _ => Err(()),
    }
}

fn expect<T: PartialEq + Display>(msg: &str, actual: T, expected: T) {
    if actual != expected {
        panic!("{} expected: {}, actual: {}", msg, expected, actual);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Test init
    {
        let client = &*CLIENT;
        let resp = client
            .get(format!("{}/init", *HOST).parse().unwrap())
            .await
            .unwrap();
        expect("init ok", resp.status(), StatusCode::OK);
    }

    // Test add and query
    add("hello".into(), "world".into()).await.unwrap();
    let value = query("hello".into()).await.unwrap();
    expect("query hello", value, "world".into());

    // Test del
    del("hello".into()).await.unwrap();
    if let Some(_) = query("hello".into()).await {
        panic!("value is not deleted");
    }

    // Test batch add and list
    let kvs: Vec<_> = (0..100)
        .map(|i| InsrtRequest {
            key: format!("key{}", i),
            value: format!("value{}", i),
        })
        .collect();
    batch(kvs).await.unwrap();
    let keys: Vec<_> = (0..100).map(|i| format!("key{}", i)).collect();
    let values = list(keys).await.unwrap();
    expect("return values length", values.len(), 100);

    for (i, value) in values.into_iter().enumerate() {
        expect(
            format!("return key {}", i).as_str(),
            value.key,
            format!("key{}", i),
        );

        expect(
            format!("return value {}", i).as_str(),
            value.value,
            format!("value{}", i),
        );
    }

    // Partial hit
    let keys: Vec<_> = (0..200).map(|i| format!("key{}", i)).collect();
    let values = list(keys).await.unwrap();
    expect("return values length", values.len(), 100);

    // All miss
    let keys: Vec<_> = (100..200).map(|i| format!("key{}", i)).collect();
    list(keys).await.unwrap_err();

    // Test zset add and zrange
    let score_values: Vec<_> = (0..100)
        .map(|i| ScoreValue {
            score: (i / 2) as u32,
            value: format!("{}", i),
        })
        .collect();
    for score_value in score_values {
        zadd("zset".into(), score_value.score, score_value.value)
            .await
            .unwrap();
    }
    let score_values = zrange("zset".into(), 0, 49).await.unwrap();
    expect("score_values length (i / 2)", score_values.len(), 100);
    for (i, score_value) in score_values.into_iter().enumerate() {
        expect(
            format!("score_value.score {}", i).as_str(),
            score_value.score,
            (i / 2) as u32,
        );
        expect(
            format!("score_value.value {}", i).as_str(),
            score_value.value,
            format!("{}", i),
        );
    }

    // Test zadd existing values
    let score_values: Vec<_> = (0..100)
        .map(|i| ScoreValue {
            score: (i * 2) as u32,
            value: format!("{}", i),
        })
        .collect();
    for score_value in score_values {
        zadd("zset".into(), score_value.score, score_value.value)
            .await
            .unwrap();
    }
    let score_values = zrange("zset".into(), 0, 198).await.unwrap();
    expect("score_values length (i * 2)", score_values.len(), 100);
    for (i, score_value) in score_values.into_iter().enumerate() {
        expect(
            format!("score_value.score {}", i).as_str(),
            score_value.score,
            (i * 2) as u32,
        );
        expect(
            format!("score_value.value {}", i).as_str(),
            score_value.value,
            format!("{}", i),
        );
    }

    // Test zrmv
    zrmv("zset".into(), "0".into()).await.unwrap();
    let score_values = zrange("zset".into(), 0, 0).await.unwrap();
    if score_values.len() != 0 {
        panic!("score_values.len() is not correct");
    }
    for i in 1..100 {
        zrmv("zset".into(), format!("{}", i)).await.unwrap();
    }
    let score_values = zrange("zset".into(), 0, 0).await.unwrap();
    expect("empty zset", score_values.len(), 0);

    // Test delete zset
    del("zset".into()).await.unwrap();
    zrange("zset".into(), 0, 0).await.unwrap_err();

    // Test overwrite zset
    zadd("zset".into(), 0, "a".into()).await.unwrap();
    let score_values = zrange("zset".into(), 0, 0).await.unwrap();
    expect("one value in zset", score_values.len(), 1);
    add("zset".into(), "foo".into()).await.unwrap();
    zadd("zset".into(), 0, "a".into()).await.unwrap_err();
    zrange("zset".into(), 0, 0).await.unwrap_err();
    let value = query("zset".into()).await.unwrap();
    expect("zset = foo", value, "foo".into());
    del("zset".into()).await.unwrap();

    println!("correct");

    // Benchmark add
    let n: usize = std::env::var("N").map_or(10000, |value| value.parse::<usize>().unwrap_or(10000));
    let kvs = (0..n)
        .map(|i| InsrtRequest {
            key: format!("key{}", i),
            value: format!("value{}", i),
        });
    let tik = Instant::now();
    let mut handles = vec![];
    for kv in kvs {
        handles.push(tokio::spawn(async move {
            add(kv.key, kv.value).await.unwrap();
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }
    let tok = Instant::now();
    let insrt_duration = tok.duration_since(tik);
    println!("N: {} insrt_duration: {:?}", n, insrt_duration);

    // Benchmark query
    let tik = Instant::now();
    let mut handles = vec![];
    for i in 0..n {
        handles.push(tokio::spawn(async move {
            query(format!("key{}", i)).await.unwrap();
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }
    let tok = Instant::now();
    let query_duration = tok.duration_since(tik);
    println!("N: {} query_duration: {:?}", n, query_duration);

    // Benchmark del
    let tik = Instant::now();
    let mut handles = vec![];
    for i in 0..n {
        handles.push(tokio::spawn(async move {
            del(format!("key{}", i)).await.unwrap();
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }
    let tok = Instant::now();
    let del_duration = tok.duration_since(tik);
    println!("N: {} del_duration: {:?}", n, del_duration);

    // Benchmark batch
    let kvs = (0..n)
        .map(|i| InsrtRequest {
            key: format!("key{}", i),
            value: format!("value{}", i),
        })
        .collect();
    let tik = Instant::now();
    batch(kvs).await.unwrap();
    let tok = Instant::now();
    let batch_duration = tok.duration_since(tik);
    println!("N: {} batch_duration: {:?}", n, batch_duration);

    // Benchmark list
    let keys: Vec<_> = (0..n)
        .map(|i| format!("key{}", i))
        .collect();
    let tik = Instant::now();
    let values = list(keys).await.unwrap();
    expect("benchmark list length", values.len(), n);
    let tok = Instant::now();
    let list_duration = tok.duration_since(tik);
    println!("N: {} list_duration: {:?}", n, list_duration);

    // Benchmark zadd
    let score_values: Vec<_> = (0..n)
        .map(|i| ScoreValue {
            score: i as u32,
            value: format!("{}", i),
        })
        .collect();
    let tik = Instant::now();
    let mut handles = vec![];
    for (i, score_value) in score_values.into_iter().enumerate() {
        handles.push(tokio::spawn(async move {
            zadd(format!("zset{}", i).into(), score_value.score, score_value.value).await.unwrap();
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }
    let tok = Instant::now();
    let zadd_duration = tok.duration_since(tik);
    println!("N: {} zadd_duration: {:?}", n, zadd_duration);

    // Benchmark zrange
    let tik = Instant::now(); 
    let mut handles = vec![];
    for i in 0..n {
        handles.push(tokio::spawn(async move {
            zrange(format!("zset{}", i).into(), i as u32, i as u32).await.unwrap();
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }
    let tok = Instant::now();
    let zrange_duration = tok.duration_since(tik);
    println!("N: {} zrange_duration: {:?}", n, zrange_duration);

    // Benchmark zrmv
    let tik = Instant::now();
    let mut handles = vec![];
    for i in 0..n {
        handles.push(tokio::spawn(async move {
            zrmv(format!("zset{}", i).into(), format!("{}", i)).await.unwrap();
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }
    let tok = Instant::now();
    let zrmv_duration = tok.duration_since(tik);
    println!("N: {} zrmv_duration: {:?}", n, zrmv_duration);

    // Cleanup
    println!("cleanup...");
    let mut handles = vec![];
    for i in 0..n {
        handles.push(tokio::spawn(async move {
            del(format!("key{}", i)).await.unwrap();
            del(format!("zset{}", i)).await.unwrap();
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }

    Ok(())
}
