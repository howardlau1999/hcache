cmake_minimum_required(VERSION 3.18)

include(ExternalProject)
include(ProcessorCount)
ProcessorCount(N)

ExternalProject_Add(librocksdb_src
    PREFIX ${CMAKE_CURRENT_BINARY_DIR}/rocksdb
    URL ${CMAKE_CURRENT_LIST_DIR}/third_party/rocksdb
    SOURCE_DIR ${librocksdb_SOURCE_DIR}
    BUILD_IN_SOURCE 1
    CONFIGURE_COMMAND ""
    BUILD_COMMAND make -j${N} static_lib
    INSTALL_COMMAND ""
    BUILD_BYPRODUCTS "${librocksdb_SOURCE_DIR}/librocksdb.a"
)

ExternalProject_Add_Step(librocksdb_src forcebuild DEPENDEES configure DEPENDERS build ALWAYS 1)
ExternalProject_Get_Property(librocksdb_src source_dir)

add_library(librocksdb STATIC IMPORTED GLOBAL)
set_property(TARGET librocksdb
    PROPERTY IMPORTED_LOCATION 
    "${source_dir}/librocksdb.a"
)

find_package(ZLIB)

add_subdirectory(third_party/seastar)
include_directories("${source_dir}/include")
add_executable(hcache src/main.cc)
add_dependencies(hcache librocksdb_src)
target_link_libraries(hcache Seastar::seastar librocksdb ZLIB::ZLIB)