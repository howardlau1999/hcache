use std::{net::SocketAddr, sync::Arc};

use crate::{http_parser::conn_dispatcher, Storage};

pub fn monoio_parser_run(storage: Arc<Storage>) {
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
            rt.block_on(async move {
                let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
                let listener = monoio::net::TcpListener::bind(addr).unwrap();
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    let storage = storage.clone();
                    monoio::spawn(async move { conn_dispatcher(stream, storage).await });
                }
            });
            println!("Http server stopped");
        });
        threads.push(thread);
    }
    for thread in threads {
        thread.join().unwrap();
    }
}
