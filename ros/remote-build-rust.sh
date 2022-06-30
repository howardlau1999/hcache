#!/usr/bin/env bash
./connect-ecs.sh "rm -rf /data && mkdir -p /data"
./connect-ecs.sh "export HTTPS_PROXY=\"${REMOTE_HTTPS_PROXY}\"; cd hcache && cargo build --release"

