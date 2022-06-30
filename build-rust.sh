#!/usr/bin/env bash
pushd client && cargo build --release && popd
pushd rust && cargo build --release && popd