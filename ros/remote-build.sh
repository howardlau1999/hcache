#!/bin/bash
set -e
BRANCH=${1:-"master"}
echo -n "GitHub Username: "
read USERNAME
echo -n "GitHub Password: "
read PASSWORD
URL="https://${USERNAME}:${PASSWORD}@hub.fastgit.xyz/howardlau1999/hcache"
./connect-ecs.sh "if [ -d hcache ]; then \
cd hcache && git checkout $BRANCH && git pull --rebase $URL $BRANCH ;\
else \
git clone $URL && cd hcache && git checkout $BRANCH ;\
fi"
./connect-ecs.sh "rm -rf /data && mkdir -p /data"
./connect-ecs.sh "cd hcache && cargo build --release && cargo build"
