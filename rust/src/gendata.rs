fn main() {
  let path = std::env::args().nth(1).unwrap();
  let prefix = std::env::args().nth(2).unwrap();
  let db = rocksdb::DB::open_default(path).unwrap();
  for i in 0..100 {
    let key = format!("{}-{}", prefix, i);
    let value = format!("{}-{}", prefix, i);
    db.put(key.as_bytes(), value.as_bytes()).unwrap();
  }
}