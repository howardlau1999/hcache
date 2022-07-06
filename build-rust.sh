#!/usr/bin/env bash
set -e
export RUSTFLAGS="-C target-cpu=native" 
pushd client && cargo build --release && popd
pushd rust && cargo build --release && popd
cp rust/target/release/hcache /usr/bin