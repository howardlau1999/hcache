#include <hcache/data_loader.h>
#include <hcache/sharded_storage.h>
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
sharded_storage hcache;

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
    folly::fbstring key_string(key.data(), key.size());
    return hcache.get_value_by_key(std::move(key_string)).then([](auto &&value) {
      auto rep = std::make_unique<seastar::httpd::reply>();
      if (value.has_value()) {
        rep->_content = std::move(seastar::sstring(value.value().data(), value.value().size()));
      } else {
        rep->set_status(seastar::reply::status_type::not_found);
      }
      rep->done();
      return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
    });
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
    return hcache.add_key_value(folly::fbstring(key.data(), key.size()), folly::fbstring(value.data(), value.size()))
        .then([] {
          auto rep = std::make_unique<seastar::httpd::reply>();
          rep->done();
          return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
        });
  };
};

class batch_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    simdjson::dom::parser parser;
    simdjson::padded_string_view json = simdjson::padded_string_view(req->content.data(), req->content.size());
    auto &&doc = parser.parse(json);
    return seastar::parallel_for_each(
               doc,
               [](auto &&kv) {
                 auto const key = kv["key"].get_string().take_value();
                 auto const value = kv["value"].get_string().take_value();
                 return hcache.add_key_value(
                     folly::fbstring(key.data(), key.size()), folly::fbstring(value.data(), value.size()));
               })
        .then([] {
          auto rep = std::make_unique<seastar::httpd::reply>();
          rep->done();
          return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
        });
  };
};

class del_handler : public seastar::httpd::handler_base {
public:
  virtual future<std::unique_ptr<seastar::httpd::reply>> handle(
      const seastar::sstring &path, std::unique_ptr<seastar::request> req,
      std::unique_ptr<seastar::httpd::reply> rep) override {
    auto const &key = req->param["k"];
    folly::fbstring key_string(folly::StringPiece(key.data(), key.size()));
    return hcache.del_key(std::move(key_string)).then([]() {
      auto rep = std::make_unique<seastar::httpd::reply>();
      rep->done();
      return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
    });
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
    return hcache.list_keys(keys).then([](auto &&result) {
      auto rep = std::make_unique<seastar::httpd::reply>();
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
    });
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

    return hcache.zset_add(folly::fbstring(key.data(), key.size()), folly::fbstring(value.data(), value.size()), score)
        .then([](auto success) {
          auto rep = std::make_unique<seastar::httpd::reply>();
          if (!success) { rep->set_status(seastar::httpd::reply::status_type::bad_request); }
          rep->done();
          return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
        });
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
    return hcache.zset_zrange(folly::fbstring(key.data(), key.size()), min_score, max_score)
        .then([](auto &&maybe_score_values) {
          auto rep = std::make_unique<seastar::httpd::reply>();
          if (maybe_score_values.has_value()) {
            auto score_values = maybe_score_values.value();
            if (score_values.empty()) {
              rep->_content = "[]";
            } else {
              auto d = rapidjson::Document();
              auto &sv_list = d.SetArray();
              auto &allocator = d.GetAllocator();
              for (auto const &sv: score_values) {
                auto const &values = sv.values;
                for (auto it = values.cbegin(); it != values.cend(); ++it) {
                  auto &&sv_object = rapidjson::Value(rapidjson::kObjectType);
                  auto &&v_string = rapidjson::Value(it->data(), it->size());
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
        });
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
    return hcache.zset_rmv(key, value).then([] {
      auto rep = std::make_unique<seastar::httpd::reply>();
      rep->done();
      return make_ready_future<std::unique_ptr<seastar::httpd::reply>>(std::move(rep));
    });
  };
};

int main(int argc, char **argv) {

  seastar::app_template app;
  seastar::httpd::http_server_control http_server;
  app.run(argc, argv, [&]() -> future<> {
    return hcache.start().then([&]() {
      init_sharded_storage(hcache);
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
            seastar::listen_options opts;
            opts.listen_backlog = 100000;
            return http_server.listen(seastar::ipv4_addr{8080}).then([]() {
              return seastar::keep_doing([] { return seastar::sleep_abortable(std::chrono::hours(1)); });
            });
          })
          .finally([&http_server] {
            applog.info("Aborting server");
            return http_server.stop();
          });
    });
  });
  return 0;
}
