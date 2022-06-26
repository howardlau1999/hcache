#!/usr/bin/env bash
set -x
export ECS_IP=$(jq -r '.TestStack | map(select(.OutputKey == "public_ip"))[0].OutputValue?' stack.outputs.json)
echo "ECS_IP=$ECS_IP"
ssh root@$ECS_IP $1
