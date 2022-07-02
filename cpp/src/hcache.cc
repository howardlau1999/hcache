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
#include <simdjson.h>

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

  void zrmv(folly::fbstring const &value) {
    auto it = value_to_score_.find(value);
    if (it == value_to_score_.end()) { return; }
    csl::Accessor accessor(score_to_values_);
    auto old_score_it = accessor.find({it->second, nullptr});
    if (old_score_it.good()) {
      old_score_it->values->erase(value);
      if (old_score_it->values->empty()) { accessor.remove({it->second, nullptr}); }
    }
  }

  void zadd(folly::fbstring const &value, uint32_t score) {
    auto it = value_to_score_.find(value);
    if (it != value_to_score_.end()) {
      csl::Accessor accessor(score_to_values_);
      // Remove from old score
      auto old_score_it = accessor.find(score_values{it->second, nullptr});
      old_score_it->values->erase(value);
      if (old_score_it->values->empty()) { accessor.remove({it->second, nullptr}); }
    }
    // Update score
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

  folly::fbvector<score_values> zrange(uint32_t min_score, uint32_t max_score) {
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
    auto zset_it = zsets_.find(key);
    if (zset_it == zsets_.end()) { return; }
    auto &zset = *zset_it->second;
    zset.zrmv(value);
  }

  folly::Optional<folly::fbvector<zset::score_values>>
  zset_zrange(folly::fbstring const &key, uint32_t min_score, uint32_t max_score) {
    auto zset_it = zsets_.find(key);
    if (zset_it != zsets_.end()) { return zset_it->second->zrange(min_score, max_score); }
    return folly::none;
  }
};

storage hcache;

class init_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    rep->_content = "ok";
    rep->done();
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
    rep->done();
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class add_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    simdjson::dom::parser parser;
    simdjson::padded_string_view json = simdjson::padded_string_view(req->content.data(), req->content.size());
    auto document = parser.parse(json);
    auto const key = document["key"].get_string().take_value();
    auto const value = document["value"].get_string().take_value();
    hcache.add_key_value(folly::fbstring(key.data(), key.size()), folly::fbstring(value.data(), value.size()));
    rep->done();
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class batch_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    simdjson::dom::parser parser;
    simdjson::padded_string_view json = simdjson::padded_string_view(req->content.data(), req->content.size());
    for (auto const &kv: parser.parse(json)) {
      auto const key = kv["key"].get_string().take_value();
      auto const value = kv["value"].get_string().take_value();
      hcache.add_key_value(folly::fbstring(key.data(), key.size()), folly::fbstring(value.data(), value.size()));
    }
    rep->done();
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
    rep->done();
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class list_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    simdjson::dom::parser parser;
    simdjson::padded_string_view json = simdjson::padded_string_view(req->content.data(), req->content.size());
    folly::F14FastSet<folly::StringPiece> keys;
    for (auto const &key: parser.parse(json)) { keys.insert(key.get_string().take_value()); }
    auto result = hcache.list_keys(keys);
    if (!result.empty()) {
      auto d = rapidjson::Document();
      auto &kv_list = d.SetArray();
      auto &allocator = d.GetAllocator();
      for (auto const &kv: result) {
        auto &&kv_object = rapidjson::Value(rapidjson::kObjectType);
        auto &&k_string = rapidjson::StringRef(kv.key.data(), kv.key.size());
        auto &&v_string = rapidjson::StringRef(kv.value.data(), kv.value.size());
        kv_object.AddMember("key", k_string, allocator);
        kv_object.AddMember("value", v_string, allocator);
        kv_list.PushBack(kv_object, allocator);
      }
      rapidjson::StringBuffer buffer;
      rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
      d.Accept(writer);
      rep->write_body("json", seastar::sstring(buffer.GetString(), buffer.GetSize()));
    } else {
      rep->set_status(seastar::httpd::reply::status_type::not_found);
    }
    rep->done();
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class zadd_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    auto const &key = req->param["k"];
    simdjson::dom::parser parser;
    simdjson::padded_string_view json = simdjson::padded_string_view(req->content.data(), req->content.size());
    auto document = parser.parse(json);
    auto const score = document["score"].get_uint64().take_value();
    auto const value = document["value"].get_string().take_value();
    if (!hcache.zset_add(folly::fbstring(key.data(), key.size()), folly::fbstring(value.data(), value.size()), score)) {
      rep->set_status(seastar::httpd::reply::status_type::bad_request);
    }
    rep->done();
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class zrange_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    auto const &key = req->param["k"];
    simdjson::dom::parser parser;
    simdjson::padded_string_view json = simdjson::padded_string_view(req->content.data(), req->content.size());
    auto document = parser.parse(json);
    auto const min_score = document["min_score"].get_uint64().take_value();
    auto const max_score = document["max_score"].get_uint64().take_value();
    auto maybe_score_values = hcache.zset_zrange(folly::fbstring(key.data(), key.size()), min_score, max_score);
    if (maybe_score_values.has_value()) {
      auto score_values = maybe_score_values.value();
      if (score_values.empty()) {
        rep->_content = "[]";
      } else {
        auto d = rapidjson::Document();
        auto &sv_list = d.SetArray();
        auto &allocator = d.GetAllocator();
        for (auto const &sv: score_values) {
          auto values_ptr = sv.values;
          for (auto it = values_ptr->cbegin(); it != values_ptr->cend(); ++it) {
            auto &&sv_object = rapidjson::Value(rapidjson::kObjectType);
            auto &&v_string = rapidjson::Value(it->first.data(), it->first.size(), allocator);
            sv_object.AddMember("score", sv.score, allocator);
            sv_object.AddMember("value", v_string, allocator);
            sv_list.PushBack(sv_object, allocator);
          }
        }
        rapidjson::StringBuffer buffer;
        rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
        d.Accept(writer);
        rep->write_body("json", seastar::sstring(buffer.GetString(), buffer.GetSize()));
      }
    } else {
      rep->set_status(seastar::httpd::reply::status_type::not_found);
    }
    rep->done();
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

class zrmv_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    auto const &key_value = req->param["kv"];
    auto slash_ptr = key_value.data();
    auto end_ptr = slash_ptr + key_value.size();
    while (slash_ptr != end_ptr) {
      if (*(slash_ptr++) == '/') { break; }
    }
    auto const key = folly::fbstring(key_value.data(), slash_ptr - key_value.data() - 1);
    auto const value = folly::fbstring(slash_ptr, end_ptr - slash_ptr);
    hcache.zset_rmv(key, value);
    rep->done();
    return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
  };
};

int main(int argc, char **argv) {
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
        applog.error("Failed to open RocksDB");
      } else {
        applog.info("RocksDB opened");
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
        applog.info("Loaded {} keys", key_count);
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

            routes.add_default_handler(new init_handler);
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
