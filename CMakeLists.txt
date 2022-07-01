cmake_minimum_required(VERSION 3.18)
project(hcache VERSION 0.1)

string(REPLACE "-O2" "-O3" CMAKE_CXX_FLAGS_RELWITHDEBINFO "${CMAKE_CXX_FLAGS_RELWITHDEBINFO}")
add_subdirectory(cpp)