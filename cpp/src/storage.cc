#include <hcache/storage.h>
#include <rocksdb/db.h>
#include <filesystem>
#include <fstream>

storage hcache;

void zset::zrmv(folly::fbstring const &value) {
  auto it = value_to_score_.find(value);
  if (it == value_to_score_.end()) { return; }
  csl::Accessor accessor(score_to_values_);
  auto old_score_it = accessor.find({it->second, nullptr});
  if (old_score_it.good()) {
    old_score_it->values->erase(value);
    if (old_score_it->values->empty()) { accessor.remove({it->second, nullptr}); }
  }
}

void zset::zadd(folly::fbstring const &value, uint32_t score) {
  auto it = value_to_score_.find(value);
  if (it != value_to_score_.end()) {
    csl::Accessor accessor(score_to_values_);
    // Remove from old score
    auto old_score_it = accessor.find(score_values{it->second, nullptr});
    old_score_it->values->erase(value);
    if (old_score_it->values->empty()) { accessor.remove({it->second, nullptr}); }
  }
  csl::Accessor accessor(score_to_values_);
  auto new_score_it = accessor.find(score_values{score, nullptr});
  if (new_score_it.good()) {
    new_score_it->values->emplace(value, unit{});
  } else {
    auto new_values = std::make_shared<folly::ConcurrentHashMap<folly::fbstring, unit>>();
    new_values->emplace(value, unit{});
    accessor.insert(score_values{score, new_values});
  }
  value_to_score_.erase(value);
  value_to_score_.emplace(value, score);
}

folly::fbvector<zset::score_values> zset::zrange(uint32_t min_score, uint32_t max_score) {
  folly::fbvector<score_values> result;
  csl::Accessor accessor(score_to_values_);
  csl::Skipper skipper(accessor);
  skipper.to({min_score, nullptr});
  while (skipper.good() && skipper->score <= max_score) {
    result.push_back(*skipper);
    ++skipper;
  }

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