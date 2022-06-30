#!/usr/bin/env bash
function close_tunnel() {
  ./connect-ecs.sh -S $SOCKET_PATTERN -O "exit"
}
set -e
BRANCH=${1:-"master"}
LOCAL_PROXY=${LOCAL_PROXY:-"192.168.237.1:10080"}
SOCKET_PATTERN="$HOME/.ssh/controlmasters/%r@%h:%p"
mkdir -p ~/.ssh/controlmasters
./connect-ecs.sh -M -S $SOCKET_PATTERN -R "10080:$LOCAL_PROXY" -N -f
trap close_tunnel EXIT
trap close_tunnel SIGINT
export REMOTE_HTTPS_PROXY="http://localhost:10080"