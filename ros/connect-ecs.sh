#!/usr/bin/env bash
set -x
export ECS_IP=$(jq -r '.TestStack | map(select(.OutputKey == "public_ip"))[0].OutputValue?' stack.outputs.json)
echo "ECS_IP=$ECS_IP"
ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null root@$ECS_IP $@
