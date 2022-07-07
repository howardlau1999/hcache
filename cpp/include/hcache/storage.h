#if !defined(_STORAGE_H_)
#define _STORAGE_H_

#include <folly/AtomicHashMap.h>
#include <folly/ConcurrentSkipList.h>
#include <folly/FBString.h>
#include <folly/FBVector.h>
#include <folly/concurrency/AtomicSharedPtr.h>
#include <folly/concurrency/ConcurrentHashMap.h>
#include <folly/container/F14Set.h>
#include <boost/container/flat_map.hpp>

struct key_value {
  key_value(folly::StringPiece const &key, folly::fbstring const &value) : key(key), value(value) {}

  folly::fbstring key;
  folly::fbstring value;
};

class zset {
  folly::SharedMutex mutex_;
public:
  struct score_values {
    score_values(uint32_t score, folly::F14ValueSet<folly::fbstring> values) : score(score), values(values)  {}
    uint32_t score{};
    folly::F14ValueSet<folly::fbstring> values;
  };

  folly::ConcurrentHashMap<folly::fbstring, uint32_t> value_to_score_;
  boost::container::flat_map<uint32_t, folly::F14ValueSet<folly::fbstring>> score_to_values_;

  void zrmv(folly::fbstring const &value);
  void zadd(folly::fbstring const &value, uint32_t score);
  folly::fbvector<score_values> zrange(uint32_t min_score, uint32_t max_score);
};

class storage {
public:
  using zset_ptr = std::shared_ptr<zset>;
  using zset_map = folly::ConcurrentHashMap<folly::fbstring, zset_ptr>;
  zset_map zsets_;
  folly::ConcurrentHashMap<folly::fbstring, folly::fbstring> kv_;

  folly::Optional<folly::fbstring> get_value_by_key(folly::fbstring &&key);
  void add_key_value(folly::fbstring const &key, folly::fbstring const &value);
  void del_key(folly::fbstring &&key);
  folly::fbvector<key_value> list_keys(folly::F14FastSet<folly::StringPiece> const &keys);
  bool zset_add(folly::fbstring const &key, folly::fbstring const &value, uint32_t score);
  void zset_rmv(folly::fbstring const &key, folly::fbstring const &value);
  folly::Optional<folly::fbvector<zset::score_values>>
  zset_zrange(folly::fbstring const &key, uint32_t min_score, uint32_t max_score);
};

extern storage hcache;

#endif// _STORAGE_H_
