#!/usr/bin/env bash
export seastar_dir=$(pwd)/seastar
cmake -Bbuild -GNinja -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_MODULE_PATH=$seastar_dir .
cmake --build build