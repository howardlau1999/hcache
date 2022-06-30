#!/bin/bash
set -e
BRANCH=${1:-"master"}
echo -n "GitHub Username: "
read USERNAME
echo -n "GitHub Password: "
read PASSWORD
URL="https://${USERNAME}:${PASSWORD}@github.com/howardlau1999/hcache"
./connect-ecs.sh "if [ -d hcache ]; then \
cd hcache && git fetch origin && git checkout $BRANCH && git pull --rebase $URL $BRANCH ;\
else \
git clone --recursive $URL && cd hcache && git checkout $BRANCH ;\
fi"
./connect-ecs.sh "rm -rf /data && mkdir -p /data"
./connect-ecs.sh "cd hcache && cargo build --release"

