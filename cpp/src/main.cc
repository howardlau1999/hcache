#include <seastar/core/app-template.hh>
#include <seastar/core/seastar.hh>
#include <seastar/util/log.hh>

using namespace seastar;
logger applog("app");

int main(int argc, char** argv) {
    seastar::app_template app;
    app.run(argc, argv, [] () -> future<> {
        applog.info("Hello world!");
        return make_ready_future<>();
    });
}
