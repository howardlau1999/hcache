#!/usr/bin/env bash
set -ex
export ECS_IP=$(jq -r '.TestStack | map(select(.OutputKey == "public_ip"))[0].OutputValue?' stack.outputs.json)
echo "ECS_IP=$ECS_IP"
scp -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null /usr/bin/clash root@${ECS_IP}:/usr/bin
scp -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -r /etc/clash root@${ECS_IP}:~/

