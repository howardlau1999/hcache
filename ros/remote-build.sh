#!/bin/bash
set -xe
BRANCH=${1:-"master"}
./connect-ecs.sh "if [ -d hcache ]; then cd hcache && git checkout $BRANCH && git pull --rebase; else git clone https://github.com/howardlau1999/hcache && git checkout $BRANCH; fi"
./connect-ecs.sh "rm -rf /data && mkdir -p /data"
./connect-ecs.sh "cd hcache && cargo build --release && cargo build"

