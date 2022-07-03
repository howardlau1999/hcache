#if !defined(_STORAGE_H_)
#define _STORAGE_H_

#include <folly/AtomicHashMap.h>
#include <folly/ConcurrentSkipList.h>
#include <folly/FBString.h>
#include <folly/FBVector.h>
#include <folly/concurrency/AtomicSharedPtr.h>
#include <folly/concurrency/ConcurrentHashMap.h>
#include <folly/container/F14Set.h>

struct key_value {
  key_value(folly::StringPiece const &key, folly::fbstring const &value) : key(key), value(value) {}

  folly::fbstring key;
  folly::fbstring value;
};

class zset {
public:
  struct unit {};
  using values_set_ptr = std::shared_ptr<folly::ConcurrentHashMap<folly::fbstring, unit>>;
  struct score_values {
    uint32_t score{};
    values_set_ptr values{nullptr};
  };

  struct score_values_less {
    bool operator()(const score_values &lhs, const score_values &rhs) const { return lhs.score < rhs.score; }
  };

  using csl = folly::ConcurrentSkipList<score_values, score_values_less>;

  folly::ConcurrentHashMap<folly::fbstring, uint32_t> value_to_score_;
  std::shared_ptr<csl> score_to_values_;

  zset() : score_to_values_(csl::createInstance()) {}

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

void init_storage();

extern storage hcache;

#endif// _STORAGE_H_
