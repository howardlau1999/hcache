#include <hcache/sharded_storage.h>
#include <seastar/core/when_all.hh>

void single_thread_zset::zrmv(folly::fbstring const &value) {
  auto it = value_to_score_.find(value);
  if (it == value_to_score_.end()) { return; }
  auto score = it->second;
  auto &values = score_to_values_[score];
  values.erase(value);
  if (values.empty()) { score_to_values_.erase(score); }
}

void single_thread_zset::zadd(folly::fbstring &&value, uint32_t score) {
  auto it = value_to_score_.find(value);
  if (it != value_to_score_.end()) {
    auto old_score = it->second;
    auto &old_values = score_to_values_[old_score];
    old_values.erase(value);
    if (old_values.empty()) { score_to_values_.erase(old_score); }
    auto &values = score_to_values_[score];
    values.emplace(value);
  }
  score_to_values_[score].emplace(value);
  value_to_score_.erase(value);
  value_to_score_.emplace(std::move(value), score);
}

folly::fbvector<single_thread_zset::score_values> single_thread_zset::zrange(uint32_t min_score, uint32_t max_score) {
  folly::fbvector<score_values> result;
  for (auto it = score_to_values_.lower_bound(min_score); it != score_to_values_.end(); ++it) {
    if (it->first > max_score) { break; }
    result.emplace_back(it->first, it->second);
  }
  return result;
}

folly::Optional<folly::fbstring> single_thread_storage::get_value_by_key(folly::fbstring &&key) {
  auto it = kv_.find(std::move(key));
  if (it != kv_.end()) { return it->second; }
  return folly::none;
}

void single_thread_storage::add_key_value(folly::fbstring &&key, folly::fbstring &&value) {
  kv_.erase(key);
  zsets_.erase(key);
  kv_.emplace(std::move(key), std::move(value));
}

void single_thread_storage::del_key(folly::fbstring &&key) {
  kv_.erase(key);
  zsets_.erase(std::move(key));
}

folly::fbvector<key_value> single_thread_storage::list_keys(folly::fbvector<folly::fbstring> const &keys) {
  folly::fbvector<key_value> result;
  for (auto &&key: keys) {
    auto it = kv_.find(key);
    if (it != kv_.end()) { result.emplace_back(std::move(key), it->second); }
  }
  return result;
}

bool single_thread_storage::zset_add(folly::fbstring &&key, folly::fbstring &&value, uint32_t score) {
  auto it = kv_.find(key);
  if (it != kv_.end()) { return false; }
  auto zset_it = zsets_.find(key);
  if (zset_it == zsets_.end()) {
    auto [new_it, _] = zsets_.emplace(std::move(key), seastar::make_lw_shared<zset>());
    zset_it = std::move(new_it);
  }
  auto &zset = *zset_it->second;
  zset.zadd(std::move(value), score);
  return true;
}

void single_thread_storage::zset_rmv(folly::fbstring const &key, folly::fbstring const &value) {
  auto zset_it = zsets_.find(key);
  if (zset_it == zsets_.end()) { return; }
  auto &zset = *zset_it->second;
  zset.zrmv(value);
}

folly::Optional<folly::fbvector<single_thread_zset::score_values>>
single_thread_storage::zset_zrange(folly::fbstring &&key, uint32_t min_score, uint32_t max_score) {
  auto zset_it = zsets_.find(std::move(key));
  if (zset_it != zsets_.end()) { return zset_it->second->zrange(min_score, max_score); }
  return folly::none;
}

seastar::future<folly::Optional<folly::fbstring>> sharded_storage::get_value_by_key(folly::fbstring &&key) {
  auto cpu = get_cpu(key);
  if (seastar::this_shard_id() == cpu) {
    return seastar::make_ready_future<folly::Optional<folly::fbstring>>(
        peers_.local().get_value_by_key(std::move(key)));
  }
  return peers_.invoke_on(cpu, &single_thread_storage::get_value_by_key, std::move(key));
}

seastar::future<> sharded_storage::add_key_value(folly::fbstring &&key, folly::fbstring &&value) {
  auto cpu = get_cpu(key);
  if (seastar::this_shard_id() == cpu) {
    peers_.local().add_key_value(std::move(key), std::move(value));
    return seastar::make_ready_future<>();
  }
  return peers_.invoke_on(cpu, &single_thread_storage::add_key_value, std::move(key), std::move(value));
}

seastar::future<> sharded_storage::del_key(folly::fbstring &&key) {
  auto cpu = get_cpu(key);
  if (seastar::this_shard_id() == cpu) {
    peers_.local().del_key(std::move(key));
    return seastar::make_ready_future<>();
  }
  return peers_.invoke_on(cpu, &single_thread_storage::del_key, std::move(key));
}

folly::fbvector<seastar::future<folly::fbvector<key_value>>>
sharded_storage::list_keys(folly::F14FastSet<folly::StringPiece> &&keys) {
  folly::fbvector<folly::fbvector<folly::fbstring>> keys_per_cpu(
      seastar::smp::count, folly::fbvector<folly::fbstring>());
  for (auto &&key: keys) {
    auto cpu = get_cpu(key);
    keys_per_cpu[cpu].emplace_back(key);
  }
  folly::fbvector<seastar::future<folly::fbvector<key_value>>> futures;
  for (int i = 0; i < keys_per_cpu.size(); ++i) {
    futures.emplace_back(
        i == seastar::this_shard_id()
            ? seastar::make_ready_future<folly::fbvector<key_value>>(peers_.local().list_keys(std::move(keys_per_cpu[i])))
            : peers_.invoke_on(i, &single_thread_storage::list_keys, std::move(keys_per_cpu[i])));
  }
  return futures;
}

seastar::future<bool> sharded_storage::zset_add(folly::fbstring &&key, folly::fbstring &&value, uint32_t score) {
  auto cpu = get_cpu(key);
  if (seastar::this_shard_id() == cpu) {
    return seastar::make_ready_future<bool>(peers_.local().zset_add(std::move(key), std::move(value), score));
  }
  return peers_.invoke_on(cpu, &single_thread_storage::zset_add, std::move(key), std::move(value), score);
}

seastar::future<> sharded_storage::zset_rmv(folly::fbstring const &key, folly::fbstring const &value) {
  auto cpu = get_cpu(key);
  if (seastar::this_shard_id() == cpu) {
    peers_.local().zset_rmv(key, value);
    return seastar::make_ready_future<>();
  }
  return peers_.invoke_on(cpu, &single_thread_storage::zset_rmv, key, value);
}

seastar::future<folly::Optional<folly::fbvector<single_thread_zset::score_values>>>
sharded_storage::zset_zrange(folly::fbstring &&key, uint32_t min_score, uint32_t max_score) {
  auto cpu = get_cpu(key);
  if (seastar::this_shard_id() == cpu) {
    return seastar::make_ready_future<folly::Optional<folly::fbvector<single_thread_zset::score_values>>>(
        peers_.local().zset_zrange(std::move(key), min_score, max_score));
  }
  return peers_.invoke_on(cpu, &single_thread_storage::zset_zrange, std::move(key), min_score, max_score);
}