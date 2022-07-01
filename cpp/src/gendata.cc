#include <iostream>
#include <rocksdb/db.h>
#include <thread>
#include <vector>

int main(int argc, char *argv[]) {
  rocksdb::DB *db;
  rocksdb::Options options;
  options.create_if_missing = true;
  rocksdb::Status status = rocksdb::DB::Open(options, argv[1], &db);
  if (!status.ok()) {
    std::cerr << status.ToString() << std::endl;
    return 1;
  }
  // 4608 MB data
  std::vector<std::thread> threads;
  for (int t = 0; t < 16; ++t) {
    threads.emplace_back([db, t]() {
      char key[33];
      char value[257];
      rocksdb::WriteBatch batch;
      for (int i = 0; i < 1024 * 1024; ++i) {
        sprintf(key, "%032d", i + 1024 * 1024 * t);
        sprintf(value, "%0256d", i + 1024 * 1024 * t);
        auto status = batch.Put(key, value);
        if (!status.ok()) {
          std::cerr << status.ToString() << std::endl;
          break;
        }
      }
      db->Write(rocksdb::WriteOptions(), &batch);
    });
  }
  for (auto && t : threads) {
    t.join();
  }
  delete db;
  return 0;
}