#include <filesystem>
#include <folly/SharedMutex.h>
#include <fstream>
#include <hcache/storage.h>
#include <rocksdb/db.h>

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
  auto it = kv_.find(std::move(key));
  if (it != kv_.end()) { return it->second; }
  return folly::none;
}

void storage::add_key_value(folly::fbstring &&key, folly::fbstring &&value) {
  kv_.erase(key);
  zsets_.erase(key);
  kv_.emplace(std::move(key), std::move(value));
}

void storage::del_key(folly::fbstring &&key) {
  kv_.erase(key);
  zsets_.erase(key);
}

folly::fbvector<key_value> storage::list_keys(folly::F14FastSet<folly::StringPiece> const& keys) {
  folly::fbvector<key_value> result;
  for (auto const &key: keys) {
    auto it = kv_.find(folly::fbstring(key));
    if (it != kv_.end()) { result.emplace_back(std::move(key), it->second); }
  }
  return result;
}

bool storage::zset_add(folly::fbstring &&key, folly::fbstring &&value, uint32_t score) {
  auto it = kv_.find(key);
  if (it != kv_.end()) { return false; }
  auto zset_it = zsets_.find(key);
  if (zset_it == zsets_.end()) {
    auto [new_it, _] = zsets_.emplace(std::move(key), std::make_shared<zset>());
    zset_it = std::move(new_it);
  }
  auto &zset = *zset_it->second;
  zset.zadd(value, score);
  return true;
}

void storage::zset_rmv(folly::fbstring &&key, folly::fbstring &&value) {
  auto zset_it = zsets_.find(std::move(key));
  if (zset_it == zsets_.end()) { return; }
  auto &zset = *zset_it->second;
  zset.zrmv(std::move(value));
}

folly::Optional<folly::fbvector<zset::score_values>>
storage::zset_zrange(folly::fbstring &&key, uint32_t min_score, uint32_t max_score) {
  auto zset_it = zsets_.find(std::move(key));
  if (zset_it != zsets_.end()) { return zset_it->second->zrange(min_score, max_score); }
  return folly::none;
}

