#!/bin/bash
set -xe
./connect-ecs.sh "if [ -d hcache ]; then cd hcache && git pull; else git clone https://github.com/howardlau1999/hcache; fi"
./connect-ecs.sh "rm -rf /data && mkdir -p /data"
./connect-ecs.sh "cd hcache && cargo build --release && cargo build"

