#!/usr/bin/env bash
set -ex
source ./get-ecs-ip.sh
scp -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null /usr/bin/clash root@${ECS_IP}:/usr/bin
scp -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -r /etc/clash root@${ECS_IP}:~/

