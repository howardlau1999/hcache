#if !defined(_SHARDED_STORAGE_H_)
#define _SHARDED_STORAGE_H_

#include <boost/container/flat_map.hpp>
#include <folly/FBString.h>
#include <folly/FBVector.h>
#include <folly/Optional.h>
#include <folly/container/F14Map.h>
#include <folly/container/F14Set.h>
#include <seastar/core/distributed.hh>

struct key_value {
  folly::fbstring key;
  folly::fbstring value;
};

class single_thread_zset {
public:
  struct score_values {
    score_values(uint32_t score, folly::F14FastSet<folly::fbstring> values) : score(score), values(values) {}
    uint32_t score{};
    folly::F14FastSet<folly::fbstring> values;
  };

  folly::F14FastMap<folly::fbstring, uint32_t> value_to_score_;
  boost::container::flat_map<uint32_t, folly::F14FastSet<folly::fbstring>> score_to_values_;

  void zrmv(folly::fbstring const &value);
  void zadd(folly::fbstring &&value, uint32_t score);
  folly::fbvector<score_values> zrange(uint32_t min_score, uint32_t max_score);
};

class single_thread_storage {
public:
  using zset = single_thread_zset;
  using zset_ptr = seastar::lw_shared_ptr<zset>;
  using zset_map = folly::F14FastMap<folly::fbstring, zset_ptr>;
  zset_map zsets_;
  folly::F14FastMap<folly::fbstring, folly::fbstring> kv_;

  folly::Optional<folly::fbstring> get_value_by_key(folly::fbstring &&key);
  void add_key_value(folly::fbstring &&key, folly::fbstring &&value);
  void del_key(folly::fbstring &&key);
  folly::fbvector<key_value> list_keys(folly::fbvector<folly::fbstring> const &keys);
  bool zset_add(folly::fbstring &&key, folly::fbstring &&value, uint32_t score);
  void zset_rmv(folly::fbstring const &key, folly::fbstring const &value);
  folly::Optional<folly::fbvector<zset::score_values>>
  zset_zrange(folly::fbstring &&key, uint32_t min_score, uint32_t max_score);
  seastar::future<> stop() { return seastar::make_ready_future<>(); }
};

class sharded_storage {
  inline unsigned get_cpu(const folly::fbstring &key) {
    return folly::hash::SpookyHashV2::Hash32(key.data(), key.size(), 19260817) % seastar::smp::count;
  }
  inline unsigned get_cpu(const folly::StringPiece &key) {
    return folly::hash::SpookyHashV2::Hash32(key.data(), key.size(), 19260817) % seastar::smp::count;
  }
  seastar::distributed<single_thread_storage> peers_;
public:
  using zset = single_thread_zset;
  seastar::future<> start() {
    return peers_.start();
  }
  seastar::future<folly::Optional<folly::fbstring>> get_value_by_key(folly::fbstring &&key);
  seastar::future<> add_key_value(folly::fbstring &&key, folly::fbstring &&value);
  seastar::future<> del_key(folly::fbstring &&key);
  folly::fbvector<seastar::future<folly::fbvector<key_value>>> list_keys(folly::F14FastSet<folly::StringPiece> &&keys);
  seastar::future<bool> zset_add(folly::fbstring &&key, folly::fbstring &&value, uint32_t score);
  seastar::future<> zset_rmv(folly::fbstring const &key, folly::fbstring const &value);
  seastar::future<folly::Optional<folly::fbvector<zset::score_values>>>
  zset_zrange(folly::fbstring &&key, uint32_t min_score, uint32_t max_score);
};

#endif// _SHARDED_STORAGE_H_
