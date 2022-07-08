#include <drogon/HttpController.h>
#include <drogon/HttpSimpleController.h>
#include <hcache/storage.h>
#include <hcache/data_loader.h>
#include <iostream>
#include <rapidjson/document.h>
#include <rapidjson/rapidjson.h>
#include <rapidjson/writer.h>
#include <simdjson.h>

using namespace drogon;

storage hcache;

class InitCtrl : public drogon::HttpSimpleController<InitCtrl> {
public:
  virtual void
  asyncHandleHttpRequest(const HttpRequestPtr &req, std::function<void(const HttpResponsePtr &)> &&callback) override {
    auto resp = HttpResponse::newHttpResponse();
    resp->setContentTypeCode(ContentType::CT_TEXT_PLAIN);
    resp->setBody("ok");
    callback(resp);
  }
  PATH_LIST_BEGIN
  PATH_ADD("/init", Get);
  PATH_LIST_END
};

class QueryCtrl : public drogon::HttpController<QueryCtrl> {
public:
  METHOD_LIST_BEGIN
  ADD_METHOD_TO(QueryCtrl::query, "/query/{1}", Get);
  METHOD_LIST_END
  void query(const HttpRequestPtr &req, std::function<void(const HttpResponsePtr &)> &&callback, const std::string &key)
      const {
    auto resp = HttpResponse::newHttpResponse();
    resp->setContentTypeCode(ContentType::CT_TEXT_PLAIN);
    auto const &value = hcache.get_value_by_key(std::move(key));
    if (value.has_value()) {
      resp->setBody(std::move(value.value().toStdString()));
    } else {
      resp->setBody("");
      resp->setStatusCode(HttpStatusCode::k404NotFound);
    }
    callback(resp);
  }
};

class DelCtrl : public drogon::HttpController<DelCtrl> {
public:
  METHOD_LIST_BEGIN
  ADD_METHOD_TO(DelCtrl::del, "/del/{1}", Get);
  METHOD_LIST_END
  void
  del(const HttpRequestPtr &req, std::function<void(const HttpResponsePtr &)> &&callback,
      const std::string &key) const {
    auto resp = HttpResponse::newHttpResponse();
    resp->setContentTypeCode(ContentType::CT_TEXT_PLAIN);
    hcache.del_key(std::move(key));
    callback(resp);
  }
};

class AddCtrl : public drogon::HttpSimpleController<AddCtrl> {
public:
  virtual void
  asyncHandleHttpRequest(const HttpRequestPtr &req, std::function<void(const HttpResponsePtr &)> &&callback) override {
    auto resp = HttpResponse::newHttpResponse();
    resp->setContentTypeCode(ContentType::CT_TEXT_PLAIN);
    simdjson::dom::parser parser;
    simdjson::padded_string_view json = simdjson::padded_string_view(req->bodyData(), req->bodyLength());
    auto document = parser.parse(json);
    auto &&key = document["key"].get_string().take_value();
    auto &&value = document["value"].get_string().take_value();
    hcache.add_key_value(std::move(folly::fbstring(key.data(), key.size())), std::move(folly::fbstring(value.data(), value.size())));
    callback(resp);
  }
  PATH_LIST_BEGIN
  PATH_ADD("/add", Post);
  PATH_LIST_END
};

class BatchCtrl : public drogon::HttpSimpleController<BatchCtrl> {
public:
  virtual void
  asyncHandleHttpRequest(const HttpRequestPtr &req, std::function<void(const HttpResponsePtr &)> &&callback) override {
    auto resp = HttpResponse::newHttpResponse();
    resp->setContentTypeCode(ContentType::CT_TEXT_PLAIN);
    simdjson::dom::parser parser;
    simdjson::padded_string_view json = simdjson::padded_string_view(req->bodyData(), req->bodyLength());
    for (auto const &kv: parser.parse(json)) {
      auto &&key = kv["key"].get_string().take_value();
      auto &&value = kv["value"].get_string().take_value();
      hcache.add_key_value(std::move(folly::fbstring(key.data(), key.size())), std::move(folly::fbstring(value.data(), value.size())));
    }
    resp->setBody("");
    callback(resp);
  }
  PATH_LIST_BEGIN
  PATH_ADD("/batch", Post);
  PATH_LIST_END
};

class ListCtrl : public drogon::HttpSimpleController<ListCtrl> {
public:
  virtual void
  asyncHandleHttpRequest(const HttpRequestPtr &req, std::function<void(const HttpResponsePtr &)> &&callback) override {
    auto resp = HttpResponse::newHttpResponse();
    resp->setContentTypeCode(ContentType::CT_APPLICATION_JSON);
    simdjson::dom::parser parser;
    simdjson::padded_string_view json = simdjson::padded_string_view(req->bodyData(), req->bodyLength());
    folly::F14FastSet<folly::StringPiece> keys;
    for (auto &&key: parser.parse(json)) { keys.emplace(key.get_string().take_value()); }
    auto result = hcache.list_keys(std::move(keys));
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
      resp->setBody(std::move(std::string(buffer.GetString(), buffer.GetSize())));
    } else {
      resp->setBody("");
      resp->setStatusCode(HttpStatusCode::k404NotFound);
    }
    callback(resp);
  }
  PATH_LIST_BEGIN
  PATH_ADD("/list", Post);
  PATH_LIST_END
};

class ZsetCtrl : public drogon::HttpController<ZsetCtrl> {
public:
  METHOD_LIST_BEGIN
  ADD_METHOD_TO(ZsetCtrl::zadd, "/zadd/{1}", Post);
  ADD_METHOD_TO(ZsetCtrl::zrange, "/zrange/{1}", Post);
  ADD_METHOD_TO(ZsetCtrl::zrmv, "/zrmv/{1}/{2}", Get);
  METHOD_LIST_END
  void zadd(const HttpRequestPtr &req, std::function<void(const HttpResponsePtr &)> &&callback, const std::string &key)
      const {
    auto resp = HttpResponse::newHttpResponse();
    resp->setContentTypeCode(ContentType::CT_TEXT_PLAIN);
    simdjson::dom::parser parser;
    simdjson::padded_string_view json = simdjson::padded_string_view(req->bodyData(), req->bodyLength());
    auto document = parser.parse(json);
    auto const score = document["score"].get_uint64().take_value();
    auto const value = document["value"].get_string().take_value();
    if (!hcache.zset_add(std::move(folly::fbstring(key.data(), key.size())), std::move(folly::fbstring(value.data(), value.size())), score)) {
      resp->setBody("");
      resp->setStatusCode(HttpStatusCode::k404NotFound);
    }
    callback(resp);
  }

  void zrange(
      const HttpRequestPtr &req, std::function<void(const HttpResponsePtr &)> &&callback,
      const std::string &key) const {
    auto resp = HttpResponse::newHttpResponse();
    resp->setContentTypeCode(ContentType::CT_APPLICATION_JSON);
    simdjson::dom::parser parser;
    simdjson::padded_string_view json = simdjson::padded_string_view(req->bodyData(), req->bodyLength());
    auto document = parser.parse(json);
    auto const min_score = document["min_score"].get_uint64().take_value();
    auto const max_score = document["max_score"].get_uint64().take_value();
    auto maybe_score_values = hcache.zset_zrange(folly::fbstring(key.data(), key.size()), min_score, max_score);
    if (maybe_score_values.has_value()) {
      auto score_values = maybe_score_values.value();
      if (score_values.empty()) {
        resp->setBody("[]");
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
        resp->setBody(std::move(std::string(buffer.GetString(), buffer.GetSize())));
      }
    } else {
      resp->setBody("");
      resp->setStatusCode(HttpStatusCode::k404NotFound);
    }

    callback(resp);
  }

  void zrmv(
      const HttpRequestPtr &req, std::function<void(const HttpResponsePtr &)> &&callback, const std::string &key,
      const std::string &value) const {
    auto resp = HttpResponse::newHttpResponse();
    resp->setContentTypeCode(ContentType::CT_TEXT_PLAIN);
    hcache.zset_rmv(std::move(key), std::move(value));
    callback(resp);
  }
};

int main() {
  init_storage(hcache);
  auto &a = app()
                .setThreadNum(4)
                .setClientMaxBodySize(512 * 1024 * 1024)
                .setClientMaxMemoryBodySize(512 * 1024)
                .setMaxConnectionNum(10000000)
                .enableDateHeader(false)
                .enableBrotli(false)
                .enableGzip(false)
                .enableServerHeader(false)
                .setLogLevel(trantor::Logger::kDebug)
                .addListener("0.0.0.0", 8080);
  a.enableReusePort();
  a.run();
  return 0;
}