use std::fmt::Display;

use hyper::{Body, Client, Method, Request, StatusCode};
mod dto;
use dto::{InsrtRequest, ScoreRange, ScoreValue};

async fn query(key: String) -> Option<String> {
    let client = Client::new();
    let resp = client
        .get(
            format!("http://localhost:8080/query/{}", key)
                .parse()
                .unwrap(),
        )
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
    let client = Client::new();
    let resp = client
        .get(
            format!("http://localhost:8080/del/{}", key)
                .parse()
                .unwrap(),
        )
        .await
        .unwrap();
    match resp.status() {
        StatusCode::OK => Ok(()),
        _ => Err(()),
    }
}

async fn add(key: String, value: String) -> Result<(), ()> {
    let client = Client::new();
    let req = InsrtRequest { key, value };
    let req = Request::builder()
        .method(Method::POST)
        .uri("http://localhost:8080/add")
        .body(Body::from(serde_json::to_string(&req).unwrap()))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    match resp.status() {
        StatusCode::OK => Ok(()),
        _ => Err(()),
    }
}

async fn batch(kvs: Vec<InsrtRequest>) -> Result<(), ()> {
    let client = Client::new();
    let req = Request::builder()
        .method(Method::POST)
        .uri("http://localhost:8080/batch")
        .body(Body::from(serde_json::to_string(&kvs).unwrap()))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    match resp.status() {
        StatusCode::OK => Ok(()),
        _ => Err(()),
    }
}

async fn list(keys: Vec<String>) -> Result<Vec<InsrtRequest>, ()> {
    let client = Client::new();
    let req = Request::builder()
        .method(Method::POST)
        .uri("http://localhost:8080/list")
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
    let client = Client::new();
    let req = ScoreValue { score, value };
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("http://localhost:8080/zadd/{}", key))
        .body(Body::from(serde_json::to_string(&req).unwrap()))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    match resp.status() {
        StatusCode::OK => Ok(()),
        _ => Err(()),
    }
}

async fn zrange(key: String, min_score: u32, max_score: u32) -> Result<Vec<ScoreValue>, ()> {
    let client = Client::new();
    let req = ScoreRange {
        min_score,
        max_score,
    };
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("http://localhost:8080/zrange/{}", key))
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
    let client = Client::new();
    let resp = client
        .get(
            format!("http://localhost:8080/zrmv/{}/{}", key, value)
                .parse()
                .unwrap(),
        )
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

    // Test zset add and zrange
    let score_values: Vec<_> = (0..100)
        .map(|i| ScoreValue {
            score: (i / 2) as u32,
            value: format!("{}", i),
        })
        .collect();
    for score_value in score_values {
        zadd("zset".into(), score_value.score, score_value.value).await.unwrap();
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
        zadd("zset".into(), score_value.score, score_value.value).await.unwrap();
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

    println!("correct");
    Ok(())
}
