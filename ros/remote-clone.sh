#!/usr/bin/env bash
set -e
BRANCH=${1:-"master"}
echo -n "GitHub Username: "
read USERNAME
echo -n "GitHub Password: "
read PASSWORD
URL="https://${USERNAME}:${PASSWORD}@github.com/howardlau1999/hcache"
./connect-ecs.sh "export HTTPS_PROXY=\"${REMOTE_HTTPS_PROXY}\"; \
if [ -d hcache ]; then \
cd hcache && git fetch origin && git checkout $BRANCH && git pull --rebase $URL $BRANCH ;\
else \
git clone --recurse-submodules -j8 $URL && cd hcache && git checkout $BRANCH ;\
fi"