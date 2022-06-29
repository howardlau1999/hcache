#!/bin/bash
set -e
BRANCH=${1:-"master"}
echo -n "GitHub Username: "
read USERNAME
echo -n "GitHub Password: "
read PASSWORD
URL="https://${USERNAME}:${PASSWORD}@ghproxy.com/https://github.com/howardlau1999/hcache"
SOCKET_PATTERN="$HOME/.ssh/controlmasters/%r@%h:%p"
mkdir -p ~/.ssh/controlmasters
./connect-ecs.sh -M -S $SOCKET_PATTERN  -R "10080:192.168.237.1:10080" -N -f
./connect-ecs.sh "export HTTPS_PROXY=http://localhost:10080; if [ -d hcache ]; then \
cd hcache && git fetch origin && git checkout $BRANCH && git pull --rebase $URL $BRANCH ;\
else \
git clone --recursive $URL && cd hcache && git checkout $BRANCH ;\
fi"
./connect-ecs.sh "rm -rf /data && mkdir -p /data"
./connect-ecs.sh "export HTTPS_PROXY=http://localhost:10080; cd hcache && cargo build --release && cargo build"
./connect-ecs.sh -S $SOCKET_PATTERN -O "exit"
