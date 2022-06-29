#include <folly/AtomicHashMap.h>
#include <folly/ConcurrentSkipList.h>
#include <folly/FBString.h>
#include <folly/FBVector.h>
#include <folly/concurrency/AtomicSharedPtr.h>
#include <folly/concurrency/ConcurrentHashMap.h>
#include <folly/container/F14Set.h>
#include <rapidjson/document.h>
#include <rapidjson/writer.h>
#include <rocksdb/db.h>

#include <boost/container/flat_map.hpp>
#include <boost/intrusive/hashtable.hpp>
#include <seastar/core/app-template.hh>
#include <seastar/core/coroutine.hh>
#include <seastar/core/reactor.hh>
#include <seastar/core/sleep.hh>
#include <seastar/http/common.hh>
#include <seastar/http/file_handler.hh>
#include <seastar/http/httpd.hh>
#include <seastar/util/log.hh>

#include <fstream>

using boost::intrusive::hashtable;
using seastar::future;
using seastar::make_ready_future;
seastar::logger applog("app");

struct key_value {
  key_value(folly::StringPiece const &key, folly::fbstring const &value) : key(key), value(value) {}

  folly::fbstring key;
  folly::fbstring value;
};

struct score_value {
  uint32_t score;
  folly::fbstring value;
};

class zset {
public:
  struct unit {};
  using values_set_ptr = std::shared_ptr<folly::ConcurrentHashMap<folly::fbstring, unit>>;
  struct score_values {
    uint32_t score_{};
    values_set_ptr values_{nullptr};
  };

  struct score_values_less {
    bool operator()(const score_values &lhs, const score_values &rhs) const { return lhs.score_ < rhs.score_; }
  };

  using csl = folly::ConcurrentSkipList<score_values, score_values_less>;

  folly::ConcurrentHashMap<folly::fbstring, uint32_t> value_to_score_;
  std::shared_ptr<csl> score_to_values_;

  folly::Optional<uint32_t> get_score_by_value(folly::fbstring const &value) {
    auto it = value_to_score_.find(value);
    if (it != value_to_score_.end()) { return it->second; }
    return folly::none;
  }

  void zadd(folly::fbstring const &value, uint32_t score) {
    auto it = value_to_score_.find(value);
    if (it != value_to_score_.end()) {
      // Remove from old score
      csl::Accessor accessor(score_to_values_);
      auto old_score_it = accessor.lower_bound(score_values{it->second, nullptr});
      old_score_it->values_->erase(value);
      auto new_score_it = accessor.lower_bound(score_values{score, nullptr});
      if (new_score_it != accessor.end()) {
        new_score_it->values_->emplace(value, unit{});
      } else {
        auto new_values = std::make_shared<folly::ConcurrentHashMap<folly::fbstring, unit>>();
        new_values->emplace(value, unit{});
        accessor.insert(score_values{score, new_values});
      }
    }
    // Update score
    value_to_score_.emplace(value, score);
  }

  folly::fbvector<values_set_ptr> zrange(uint32_t min_score, uint32_t max_score) {
    folly::fbvector<values_set_ptr> result;
    csl::Accessor accessor(score_to_values_);
    csl::Skipper skipper(accessor);
    if (skipper.to({min_score, nullptr})) {
      while (skipper.good() && skipper->score_ <= max_score) {
        result.push_back(skipper->values_);
        ++skipper;
      }
    }

    return {};
  }
};

class storage {
public:
  using zset_ptr = std::shared_ptr<zset>;
  using zset_map = folly::ConcurrentHashMap<folly::fbstring, zset_ptr>;
  zset_map zsets_;
  folly::ConcurrentHashMap<folly::fbstring, folly::fbstring> kv_;

  folly::Optional<folly::fbstring> get_value_by_key(folly::fbstring &&key) {
    auto it = kv_.find(key);
    if (it != kv_.end()) { return it->second; }
    return folly::none;
  }

  void add_key_value(folly::fbstring const &key, folly::fbstring const &value) {
    kv_.emplace(key, value);
    zsets_.erase(key);
  }

  void del_key(folly::fbstring &&key) {
    kv_.erase(key);
    zsets_.erase(key);
  }

  folly::fbvector<key_value> list_keys(folly::F14FastSet<folly::StringPiece> const &keys) {
    folly::fbvector<key_value> result;
    for (auto const &key: keys) {
      auto it = kv_.find(folly::fbstring(key));
      if (it != kv_.end()) { result.emplace_back(key, it->second); }
    }
    return result;
  }

  bool zset_add(folly::fbstring const &key, folly::fbstring const &value, uint32_t score) {
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

  void zset_rmv(folly::fbstring const &key, folly::fbstring const &value) {
    auto it = kv_.find(key);
    if (it != kv_.end()) { return; }
    auto zset_it = zsets_.find(key);
    if (zset_it == zsets_.end()) { return; }
    auto &zset = *zset_it->second;
    zset.zadd(value, 0);
  }
};

storage hcache;

class init_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    rep->_content = "ok";
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class query_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    auto const &key = req->param["k"];
    folly::fbstring key_string(folly::StringPiece(key.data(), key.size()));
    auto const &value = hcache.get_value_by_key(std::move(key_string));
    if (value.has_value()) {
      rep->_content = std::move(seastar::sstring(value.value().data(), value.value().size()));
    } else {
      rep->set_status(seastar::reply::status_type::not_found);
    }
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class add_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    rapidjson::Document document;
    document.Parse(req->content.data(), req->content.size());
    auto const key = document["key"].GetString();
    auto const value = document["value"].GetString();
    hcache.add_key_value(key, value);
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class batch_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    rapidjson::Document document;
    document.Parse(req->content.data(), req->content.size());
    for (auto const &kv: document.GetArray()) {
      auto const key = kv["key"].GetString();
      auto const value = kv["value"].GetString();
      hcache.add_key_value(key, value);
    }
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class del_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    auto const &key = req->param["k"];
    folly::fbstring key_string(folly::StringPiece(key.data(), key.size()));
    hcache.del_key(std::move(key_string));
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class list_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    rapidjson::Document document;
    document.Parse(req->content.data(), req->content.size());
    folly::F14FastSet<folly::StringPiece> keys;
    for (auto const &key: document.GetArray()) { keys.insert(key.GetString()); }
    auto result = hcache.list_keys(keys);
    if (!result.empty()) {
      seastar::lw_shared_ptr<bool> first = seastar::make_lw_shared<bool>(true);
      rep->write_body("json", [result = std::move(result), first](seastar::output_stream<char> &&writer) -> future<> {
        return seastar::do_with(
            std::move(writer), result, first,
            [](seastar::output_stream<char> &body_writer, folly::fbvector<key_value> const &kvs,
               seastar::lw_shared_ptr<bool> first) -> future<> {
              return seastar::do_for_each(
                         kvs,
                         [&body_writer, first](key_value const &kv) -> future<> {
                           const char *prefix = *first ? "[{" : "},{";
                           *first = false;
                           rapidjson::StringBuffer k_buffer;
                           rapidjson::Writer<rapidjson::StringBuffer> k_writer(k_buffer);
                           rapidjson::Document k(rapidjson::kStringType);
                           k.SetString(rapidjson::StringRef(kv.key.data(), kv.key.size()));
                           k.Accept(k_writer);
                           rapidjson::StringBuffer v_buffer;
                           rapidjson::Writer<rapidjson::StringBuffer> v_writer(v_buffer);
                           rapidjson::Document v(rapidjson::kStringType);
                           v.SetString(rapidjson::StringRef(kv.value.data(), kv.value.size()));
                           v.Accept(v_writer);
                           return seastar::do_with(
                               std::move(k_buffer), std::move(v_buffer),
                               [&body_writer, prefix](auto const &k_buf, auto const &v_buf) -> future<> {
                                 return body_writer.write(prefix)
                                     .then([&]() -> future<> { return body_writer.write("\"key\":"); })
                                     .then([&]() -> future<> { return body_writer.write(k_buf.GetString()); })
                                     .then([&]() -> future<> { return body_writer.write(",\"value\":"); })
                                     .then([&]() -> future<> { return body_writer.write(v_buf.GetString()); });
                               });
                         })
                  .then([&body_writer]() -> future<> { return body_writer.write("}]"); })
                  .then([&]() -> future<> { return body_writer.close(); });
            });
      });
    } else {
      rep->set_status(seastar::httpd::reply::status_type::not_found);
    }
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class zadd_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    auto const& key = req->param["key"];
    rapidjson::Document document;
    document.Parse(req->content.data(), req->content.size());
    auto const score = document["score"].GetUint();
    auto const value = document["value"].GetString();
    if (!hcache.zset_add(folly::fbstring(key.data(), key.size()), value, score)) {
      rep->set_status(seastar::httpd::reply::status_type::bad_request);
    }
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class zrange_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    auto const& key = req->param["key"];
    rapidjson::Document document;
    document.Parse(req->content.data(), req->content.size());
    auto const min_score = document["min_score"].GetUint();
    auto const max_score = document["max_score"].GetUint();
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class zrmv_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    auto const& key_value = req->param["kv"];
    auto slash_ptr = key_value.data();
    auto end_ptr = slash_ptr + key_value.size();
    while (slash_ptr != end_ptr) {
      if (*(slash_ptr++) == '/') {
        break;
      }
    }
    auto const key = folly::fbstring(end_ptr, slash_ptr - key_value.data() + 1);
    auto const value = folly::fbstring(slash_ptr + 1, end_ptr - slash_ptr);
    hcache.zset_rmv(key, value);
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

int main(int argc, char **argv) {
  if (argc >= 2) {
    auto db_path = std::filesystem::path(argv[1]);
    auto marker_path = db_path / ".loaded";
    if (!std::filesystem::exists(marker_path)) {
      rocksdb::Options options;
      options.create_if_missing = true;
      options.allow_mmap_reads = true;
      options.allow_mmap_writes = true;
      options.unordered_write = true;
      options.use_adaptive_mutex = true;
      rocksdb::DB *db;
      auto status = rocksdb::DB::Open(options, argv[1], &db);
      if (!status.ok()) {
        applog.error("Failed to open RocksDB");
      } else {
        rocksdb::ReadOptions read_options;
        read_options.adaptive_readahead = true;
        read_options.async_io = true;
        read_options.verify_checksums = false;
        auto iter = db->NewIterator(read_options);
        iter->SeekToFirst();
        while (iter->Valid()) {
          auto key = iter->key();
          auto value = iter->value();
          hcache.add_key_value(folly::fbstring(key.ToString()), folly::fbstring(value.ToString()));
          iter->Next();
        }
        std::ofstream _marker(marker_path);
      }
    }
  }

  seastar::app_template app;
  seastar::httpd::http_server_control http_server;
  app.run(argc, argv, [&http_server]() -> future<> {
    return http_server.start("hcache")
        .then([&http_server] {
          applog.info("Setting routes");
          return http_server.set_routes([](seastar::httpd::routes &routes) {
            // init
            routes.put(seastar::httpd::GET, "/init", new init_handler);

            // kv: query add batch del list
            routes.add(seastar::httpd::GET, seastar::httpd::url("/query").remainder("k"), new query_handler);
            routes.put(seastar::httpd::POST, "/add", new add_handler);
            routes.put(seastar::httpd::POST, "/batch", new batch_handler);
            routes.add(seastar::httpd::GET, seastar::httpd::url("/del").remainder("k"), new del_handler);
            routes.put(seastar::httpd::POST, "/list", new list_handler);

            // zset: zadd zrmv zrange
            routes.add(seastar::httpd::POST, seastar::httpd::url("/zadd").remainder("k"), new zadd_handler);
            routes.add(seastar::httpd::POST, seastar::httpd::url("/zrange").remainder("k"), new zrange_handler);
            routes.add(seastar::httpd::GET, seastar::httpd::url("/zrmv").remainder("kv"), new zrmv_handler);
          });
        })
        .then([&http_server] {
          applog.info("Starting hcache server at :8080");
          return http_server.listen(seastar::ipv4_addr{8080}).then([]() {
            return seastar::keep_doing([] { return seastar::sleep_abortable(std::chrono::hours(1)); });
          });
        })
        .finally([&http_server] {
          applog.info("Aborting server");
          return http_server.stop();
        });
  });
  return 0;
}
