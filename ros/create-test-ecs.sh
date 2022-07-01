#!/usr/bin/env bash
set -xe
ros-cdk deploy TestStack --sync=true --outputs-file=true
export ECS_IP=$(jq -r '.TestStack | map(select(.OutputKey == "public_ip"))[0].OutputValue?' stack.outputs.json)
echo "ECS_IP=$ECS_IP"
export INSTANCE_ID=$(jq -r '.TestStack | map(select(.OutputKey == "instance_id"))[0].OutputValue?' stack.outputs.json)
echo "INSTANCE_ID=$INSTANCE_ID"
aliyun ecs ModifyInstanceVncPasswd --InstanceId=$INSTANCE_ID --VncPassword=Hca123
ssh-keygen -f "~/.ssh/known_hosts" -R "$ECS_IP" || true
