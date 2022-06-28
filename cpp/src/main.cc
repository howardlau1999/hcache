#include <folly/AtomicHashMap.h>
#include <folly/String.h>
#include <rocksdb/db.h>

#include <seastar/core/app-template.hh>
#include <seastar/core/seastar.hh>
#include <seastar/http/common.hh>
#include <seastar/http/file_handler.hh>
#include <seastar/http/function_handlers.hh>
#include <seastar/http/httpd.hh>
#include <seastar/util/log.hh>

using namespace seastar;
logger applog("app");

int main(int argc, char** argv) {
  seastar::app_template app;
  httpd::http_server_control http_server;
  app.run(argc, argv, [&http_server]() -> future<> {
    applog.info("Starting hcache server at :8080");
    return http_server.start("hcache")
        .then([&http_server] {
          return http_server.set_routes([](seastar::httpd::routes& routes) {
            routes.put(seastar::httpd::operation_type::GET, "/init",
                       new seastar::httpd::function_handler(
                           [](seastar::httpd::const_req& req,
                              seastar::httpd::reply&) -> seastar::sstring {
                             return "ok";
                           },
                           "txt"));
          });
        })
        .then([&http_server] {
          return http_server.listen(ipv4_addr{8080}).then([] {

          });
        });
  });
}
