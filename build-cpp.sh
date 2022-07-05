#!/usr/bin/env bash
set -e
CMAKE_BUILD_TYPE=${CMAKE_BUILD_TYPE:-"RelWithDebInfo"}
BUILD_DIR=cmake-build-$(echo $CMAKE_BUILD_TYPE | tr "[:upper:]" "[:lower:]")
cmake -DCMAKE_BUILD_TYPE=$CMAKE_BUILD_TYPE -GNinja -B$BUILD_DIR -DCMAKE_EXPORT_COMPILE_COMMANDS=ON $@ .
cmake --build $BUILD_DIR
cp $BUILD_DIR/cpp/hcache /usr/bin