#if !defined(_DATA_LOADER_H_)
#define _DATA_LOADER_H_

#include <filesystem>
#include <folly/FBVector.h>
#include <folly/FBString.h>
#include <fstream>
#include <rocksdb/db.h>
#include <seastar/core/future.hh>
#include <seastar/core/when_all.hh>

template<class Storage>
void init_sharded_storage(Storage &cache) {
  auto init_dir_ptr = std::getenv("INIT_DIR");
  if (init_dir_ptr) {
    auto db_path = std::filesystem::path(init_dir_ptr);
    auto marker_path = db_path / ".loaded";
    if (!std::filesystem::exists(marker_path)) {
      rocksdb::Options options;
      options.create_if_missing = true;
      options.allow_mmap_reads = true;
      options.allow_mmap_writes = true;
      options.unordered_write = true;
      options.use_adaptive_mutex = true;
      rocksdb::DB *db;
      auto status = rocksdb::DB::Open(options, db_path, &db);
      if (!status.ok()) {
        return;
      } else {
        rocksdb::ReadOptions read_options;
        read_options.readahead_size = 128 * 1024 * 1024;
        read_options.async_io = true;
        read_options.verify_checksums = false;
        auto iter = db->NewIterator(read_options);
        int key_count = 0;
        iter->SeekToFirst();
        folly::fbvector<seastar::future<>> futures;
        while (iter->Valid()) {
          auto key = iter->key();
          auto value = iter->value();
          futures.emplace_back(cache.add_key_value(folly::fbstring(key.ToString()), folly::fbstring(value.ToString())));
          ++key_count;
          iter->Next();
        }
        seastar::when_all(futures.begin(), futures.end()).get();

        delete iter;
        std::ofstream _marker(marker_path);
        db->Close();
        delete db;
      }
    }
  }
}

template<class Storage>
void init_storage(Storage &cache) {
  auto init_dir_ptr = std::getenv("INIT_DIR");
  if (init_dir_ptr) {
    auto db_path = std::filesystem::path(init_dir_ptr);
    auto marker_path = db_path / ".loaded";
    if (!std::filesystem::exists(marker_path)) {
      rocksdb::Options options;
      options.create_if_missing = true;
      options.allow_mmap_reads = true;
      options.allow_mmap_writes = true;
      options.unordered_write = true;
      options.use_adaptive_mutex = true;
      rocksdb::DB *db;
      auto status = rocksdb::DB::Open(options, db_path, &db);
      if (!status.ok()) {
        return;
      } else {
        rocksdb::ReadOptions read_options;
        read_options.readahead_size = 128 * 1024 * 1024;
        read_options.async_io = true;
        read_options.verify_checksums = false;
        auto iter = db->NewIterator(read_options);
        int key_count = 0;
        iter->SeekToFirst();
        while (iter->Valid()) {
          auto key = iter->key();
          auto value = iter->value();
          cache.add_key_value(folly::fbstring(key.ToString()), folly::fbstring(value.ToString()));
          ++key_count;
          iter->Next();
        }

        delete iter;
        std::ofstream _marker(marker_path);
        db->Close();
        delete db;
      }
    }
  }
}

#endif// _DATA_LOADER_H_
