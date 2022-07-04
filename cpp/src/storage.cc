#include <filesystem>
#include <folly/SharedMutex.h>
#include <fstream>
#include <hcache/storage.h>
#include <rocksdb/db.h>

storage hcache;

void zset::zrmv(folly::fbstring const &value) {
  auto it = value_to_score_.find(value);
  if (it == value_to_score_.end()) { return; }
  mutex_.lock();
  auto score = it->second;
  auto &values = score_to_values_[score];
  values.erase(value);
  if (values.empty()) { score_to_values_.erase(score); }
  mutex_.unlock();
}

void zset::zadd(folly::fbstring const &value, uint32_t score) {
  auto it = value_to_score_.find(value);
  if (it != value_to_score_.end()) {
    mutex_.lock();
    auto old_score = it->second;
    auto &old_values = score_to_values_[old_score];
    old_values.erase(value);
    if (old_values.empty()) { score_to_values_.erase(old_score); }
    mutex_.unlock();
  }
  {
    mutex_.lock();
    auto &values = score_to_values_[score];
    values.emplace(value);
    mutex_.unlock();
  }
  value_to_score_.erase(value);
  value_to_score_.emplace(value, score);
}

folly::fbvector<zset::score_values> zset::zrange(uint32_t min_score, uint32_t max_score) {
  folly::fbvector<score_values> result;
  mutex_.lock_shared();
  for (auto it = score_to_values_.lower_bound(min_score); it != score_to_values_.end(); ++it) {
    if (it->first > max_score) { break; }
    result.emplace_back(it->first, it->second);
  }
  mutex_.unlock_shared();
  return result;
}

folly::Optional<folly::fbstring> storage::get_value_by_key(folly::fbstring &&key) {
  auto it = kv_.find(key);
  if (it != kv_.end()) { return it->second; }
  return folly::none;
}

void storage::add_key_value(folly::fbstring const &key, folly::fbstring const &value) {
  kv_.emplace(key, value);
  zsets_.erase(key);
}

void storage::del_key(folly::fbstring &&key) {
  kv_.erase(key);
  zsets_.erase(key);
}

folly::fbvector<key_value> storage::list_keys(folly::F14FastSet<folly::StringPiece> const &keys) {
  folly::fbvector<key_value> result;
  for (auto const &key: keys) {
    auto it = kv_.find(folly::fbstring(key));
    if (it != kv_.end()) { result.emplace_back(key, it->second); }
  }
  return result;
}

bool storage::zset_add(folly::fbstring const &key, folly::fbstring const &value, uint32_t score) {
  auto it = kv_.find(key);
  if (it != kv_.end()) { return false; }
  auto zset_it = zsets_.find(key);
  if (zset_it == zsets_.end()) {
    auto [new_it, _] = zsets_.emplace(key, std::make_shared<zset>());
    zset_it = std::move(new_it);
  }
  auto &zset = *zset_it->second;
  zset.zadd(value, score);
  return true;
}

void storage::zset_rmv(folly::fbstring const &key, folly::fbstring const &value) {
  auto zset_it = zsets_.find(key);
  if (zset_it == zsets_.end()) { return; }
  auto &zset = *zset_it->second;
  zset.zrmv(value);
}

folly::Optional<folly::fbvector<zset::score_values>>
storage::zset_zrange(folly::fbstring const &key, uint32_t min_score, uint32_t max_score) {
  auto zset_it = zsets_.find(key);
  if (zset_it != zsets_.end()) { return zset_it->second->zrange(min_score, max_score); }
  return folly::none;
}

void init_storage() {
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
          hcache.add_key_value(folly::fbstring(key.ToString()), folly::fbstring(value.ToString()));
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