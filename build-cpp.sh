#!/usr/bin/env bash
CMAKE_BUILD_TYPE=${CMAKE_BUILD_TYPE:-"RelWithDebInfo"}
BUILD_DIR=cmake-build-$(echo $CMAKE_BUILD_TYPE | tr "[:upper:]" "[:lower:]")
cmake -DRELWITHDEBINFO_FORCE_OPTIMIZATION_O3=ON -DCMAKE_BUILD_TYPE=$CMAKE_BUILD_TYPE -GNinja -B$BUILD_DIR .
cmake --build $BUILD_DIR
