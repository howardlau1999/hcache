#!/bin/bash
function close_tunnel() {
  ./connect-ecs.sh -S $SOCKET_PATTERN -O "exit"
}
set -e
BRANCH=${1:-"master"}
LOCAL_PROXY=${LOCAL_PROXY:-"192.168.237.1:10080"}
echo -n "GitHub Username: "
read USERNAME
echo -n "GitHub Password: "
read PASSWORD
URL="https://${USERNAME}:${PASSWORD}@github.com/howardlau1999/hcache"
SOCKET_PATTERN="$HOME/.ssh/controlmasters/%r@%h:%p"
mkdir -p ~/.ssh/controlmasters
./connect-ecs.sh -M -S $SOCKET_PATTERN -R "10080:$LOCAL_PROXY" -N -f
trap close_tunnel EXIT
trap close_tunnel SIGINT
./connect-ecs.sh "export HTTPS_PROXY=http://localhost:10080; if [ -d hcache ]; then \
cd hcache && git fetch origin && git checkout $BRANCH && git pull --rebase $URL $BRANCH ;\
else \
git clone --recursive $URL && cd hcache && git checkout $BRANCH ;\
fi"
./connect-ecs.sh "rm -rf /data && mkdir -p /data"
./connect-ecs.sh "export HTTPS_PROXY=http://localhost:10080; export NO_PROXY=*.tsinghua.edu.cn; cd hcache && ./get-cpp-deps.sh && ./build-cpp.sh"
