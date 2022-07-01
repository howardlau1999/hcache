#!/usr/bin/env bash
if [ -z $HCACHE_IMAGE_ID ]; then
  echo -n "Please specify HCACHE_IMAGE_ID: "
  read HCACHE_IMAGE_ID
fi
set -xe
export STACK_NAME="BenchmarkStack"
export IMAGE="custom"
export HCACHE_IMAGE_ID
ros-cdk deploy $STACK_NAME --sync=true --outputs-file=true
source ./get-ecs-ip.sh
export INSTANCE_ID=$(jq -r ".$STACK_NAME | map(select(.OutputKey == \"instance_id\"))[0].OutputValue?" stack.outputs.json)
echo "INSTANCE_ID=$INSTANCE_ID"
aliyun ecs ModifyInstanceVncPasswd --InstanceId=$INSTANCE_ID --VncPassword=Hca123
ssh-keygen -f "~/.ssh/known_hosts" -R "$ECS_IP" || true
