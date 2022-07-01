#!/usr/bin/env bash
set -e
pushd client && cargo build --release && popd
pushd rust && cargo build --release && popd
cp rust/target/release/hcache /usr/bin