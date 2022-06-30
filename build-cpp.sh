#!/usr/bin/env bash
set -xe
cd cpp/third_party/seastar && sudo ./install-dependencies.sh
cd cpp/third_party/folly && sudo ./build/fbcode_builder/getdeps.py install-system-deps --recursive