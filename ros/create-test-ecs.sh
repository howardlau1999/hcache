#!/usr/bin/env bash
set -x
ros-cdk deploy TestStack --sync=true --outputs-file=true
export ECS_IP=$(jq -r '.TestStack | map(select(.OutputKey == "public_ip"))[0].OutputValue?' stack.outputs.json)
echo "ECS_IP=$ECS_IP"
ssh-keygen -f "~/.ssh/known_hosts" -R "$ECS_IP"
ssh root@$ECS_IP
