cmake_minimum_required(VERSION 3.18)
project("hcache")

add_subdirectory(third_party/seastar)
add_executable(hcache src/main.cc)
target_link_libraries(hcache Seastar::seastar)