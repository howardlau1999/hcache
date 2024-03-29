cmake_minimum_required(VERSION 3.18)
project(hcache VERSION 0.1)

include(ExternalProject)
include(ProcessorCount)
ProcessorCount(N)

ExternalProject_Add(rocksdb_src
    PREFIX ${CMAKE_CURRENT_BINARY_DIR}/rocksdb
    URL ${CMAKE_CURRENT_LIST_DIR}/third_party/rocksdb
    BUILD_IN_SOURCE 1
    CONFIGURE_COMMAND ""
    BUILD_COMMAND make -j${N} static_lib
    INSTALL_COMMAND ""
    BUILD_BYPRODUCTS "${CMAKE_CURRENT_BINARY_DIR}/rocksdb/src/rocksdb_src/librocksdb.a"
)

ExternalProject_Add_Step(rocksdb_src forcebuild DEPENDEES configure DEPENDERS build ALWAYS 1)
ExternalProject_Get_Property(rocksdb_src source_dir)

add_library(rocksdb STATIC IMPORTED GLOBAL)
set_property(TARGET rocksdb
    PROPERTY IMPORTED_LOCATION
    "${source_dir}/librocksdb.a"
)
include_directories("${source_dir}/include")

include(CheckCXXCompilerFlag)
CHECK_CXX_COMPILER_FLAG("-march=native" COMPILER_SUPPORTS_MARCH_NATIVE)
if(COMPILER_SUPPORTS_MARCH_NATIVE)
    set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -march=native")
endif()

include(third_party/seastar/cmake/Finddpdk.cmake)
find_package(ZLIB)
find_package(Boost)
find_package(Threads)
if(TARGET dpdk::dpdk)
    set(Seastar_DPDK ON)
endif(TARGET dpdk::dpdk)
add_subdirectory(third_party/seastar)
add_subdirectory(third_party/simdjson)
add_subdirectory(third_party/folly)
set(BUILD_EXAMPLES OFF)
set(BUILD_TESTING OFF)
add_subdirectory(third_party/drogon)
include_directories(third_party/rapidjson/include)
include_directories(include)

add_library(storage src/storage.cc include/hcache/storage.h include/hcache/data_loader.h)
add_dependencies(storage rocksdb_src)
target_link_libraries(storage folly Boost::boost rocksdb)

add_library(sharded_storage src/sharded_storage.cc include/hcache/sharded_storage.h include/hcache/data_loader.h)
add_dependencies(sharded_storage rocksdb_src)
target_link_libraries(sharded_storage seastar folly Boost::boost rocksdb)

add_executable(hcache src/hcache.cc)
add_dependencies(hcache rocksdb_src)
target_link_libraries(hcache sharded_storage simdjson seastar glog rocksdb folly ZLIB::ZLIB zstd snappy Boost::boost)

add_executable(hcache_shared src/hcache_shared.cc)
add_dependencies(hcache_shared rocksdb_src)
target_link_libraries(hcache_shared storage simdjson seastar glog rocksdb folly ZLIB::ZLIB zstd snappy Boost::boost)

add_executable(drogon-hcache src/drogon.cc)
add_dependencies(drogon-hcache rocksdb_src)
target_link_libraries(drogon-hcache drogon storage simdjson seastar glog rocksdb folly ZLIB::ZLIB zstd snappy Boost::boost)

if(TARGET dpdk::dpdk)
    add_library(picohttpparser src/picohttpparser.c include/picohttpparser/picohttpparser.h)
    add_executable(fstack-hcache src/fstack.cc)
    add_dependencies(fstack-hcache rocksdb_src)
    target_link_libraries(fstack-hcache picohttpparser seastar fstack storage simdjson rocksdb folly ZLIB::ZLIB zstd snappy Boost::boost)
endif(TARGET dpdk::dpdk)


add_executable(bench-client src/client.cc)
target_link_libraries(bench-client seastar)

add_executable(gendata src/gendata.cc)
add_dependencies(gendata rocksdb_src)
target_link_libraries(gendata rocksdb Threads::Threads ZLIB::ZLIB zstd snappy uring lz4 dl) 
