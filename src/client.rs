use hyper::{Body, Client, Request, StatusCode};
mod dto;
use dto::InsrtRequest;

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

async fn add(key: String, value: String) -> Result<(), ()> {
    let client = Client::new();
    let req = InsrtRequest { key, value };
    let req = Request::builder()
        .method("POST")
        .uri("http://localhost:8080/add".parse::<String>().unwrap())
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
        .method("POST")
        .uri("http://localhost:8080/batch".parse::<String>().unwrap())
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
        .method("POST")
        .uri("http://localhost:8080/list".parse::<String>().unwrap())
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    add("hello".into(), "world".into()).await.unwrap();
    let value = query("hello".into()).await.unwrap();
    if value != "world" {
        panic!("value is not correct");
    }

    let kvs: Vec<_> = (0..100).map(|i| InsrtRequest {
        key: format!("key{}", i),
        value: format!("value{}", i),
    }).collect();
    batch(kvs).await.unwrap();
    let keys: Vec<_> =  (0..100).map(|i| format!("key{}", i)).collect();
    let values = list(keys).await.unwrap();
    if values.len() != 100 {
        panic!("values.len() is not correct");
    }
    for (i, value) in values.into_iter().enumerate() {
        if value.key != format!("key{}", i) {
            panic!("value.key is not correct");
        }
        if value.value != format!("value{}", i) {
            panic!("value.value is not correct");
        }
    }
    println!("correct");
    Ok(())
}
