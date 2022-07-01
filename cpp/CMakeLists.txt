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

ExternalProject_Add(folly_src
    PREFIX ${CMAKE_CURRENT_BINARY_DIR}/folly
    URL ${CMAKE_CURRENT_LIST_DIR}/third_party/folly
    BUILD_IN_SOURCE 1
    CONFIGURE_COMMAND ""
    PATCH_COMMAND ls -1 ./build/fbcode_builder/manifests/ | xargs -I {} sed -i s~https://github.com~https://ghproxy.com/https://github.com~ ./build/fbcode_builder/manifests/{}
    BUILD_COMMAND python3 ./build/fbcode_builder/getdeps.py --allow-system-packages --scratch-path ${CMAKE_CURRENT_BINARY_DIR}/folly build folly
    INSTALL_COMMAND ""
    BUILD_BYPRODUCTS "${CMAKE_CURRENT_BINARY_DIR}/folly/installed/folly/lib/libfolly.a"
)

ExternalProject_Add_Step(rocksdb_src forcebuild DEPENDEES configure DEPENDERS build ALWAYS 1)

add_library(folly STATIC IMPORTED GLOBAL)
set_property(TARGET folly
    PROPERTY IMPORTED_LOCATION
    "${CMAKE_CURRENT_BINARY_DIR}/folly/installed/folly/lib/libfolly.a"
)
include_directories("${CMAKE_CURRENT_BINARY_DIR}/folly/installed/folly/include")

find_package(ZLIB)
find_package(Boost)
find_package(glog)
add_subdirectory(third_party/seastar)
add_subdirectory(third_party/simdjson)
include_directories(third_party/rapidjson/include)

add_executable(hcache src/hcache.cc)
add_dependencies(hcache rocksdb_src folly_src)
target_link_libraries(hcache simdjson seastar glog::glog rocksdb folly ZLIB::ZLIB zstd snappy Boost::boost)

add_executable(client src/client.cc)
target_link_libraries(client seastar)