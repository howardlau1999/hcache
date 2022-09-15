fn main() {
  let path = std::env::args().nth(1).unwrap();
  let prefix = std::env::args().nth(2).unwrap();
  let db = rocksdb::DB::open_default(path).unwrap();
  for i in 0..30000000 {
    let key = format!("{}-{}", prefix, i);
    let value = format!("{}-{}", prefix, i);
    let mut opt = rocksdb::WriteOptions::default();
    opt.disable_wal(true);
    db.put_opt(key.as_bytes(), value.as_bytes(), &opt).unwrap();
  }
  db.flush();
}
