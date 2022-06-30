#!/usr/bin/env bash
set -xe
pushd cpp/third_party/seastar && sudo ./install-dependencies.sh && popd
pushd cpp/third_party/folly && sudo ./build/fbcode_builder/getdeps.py install-system-deps --recursive && popd
