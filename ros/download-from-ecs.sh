#!/usr/bin/env bash
set -ex
source ./get-ecs-ip.sh
scp -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -r root@${ECS_IP}:$1 $2

