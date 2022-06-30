#!/usr/bin/env bash
set -xe
./connect-ecs.sh "rm -rf /data && mkdir -p /data"
./connect-ecs.sh "export HTTPS_PROXY=\"${REMOTE_HTTPS_PROXY}\"; cd hcache && ./get-cpp-deps.sh && ./build-cpp.sh"

