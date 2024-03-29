#!/usr/bin/env bash
if [ -z $HCACHE_IMAGE_ID ]; then
  echo -n "Please specify HCACHE_IMAGE_ID: "
  read HCACHE_IMAGE_ID
fi
if [ -f stacks.output.json ]; then
  echo "Found TestStack outputs json, backing up to test.stacks.output.json"
  mv stacks.output.json test.stacks.output.json
fi
set -xe
export STACK_NAME="BenchmarkStack"
export IMAGE="custom"
export HCACHE_IMAGE_ID
ros-cdk deploy $STACK_NAME --sync=true --outputs-file=true
source ./get-ecs-ip.sh
export INSTANCE_ID=$(jq -r ".$STACK_NAME | map(select(.OutputKey == \"instance_id\"))[0].OutputValue?" stack.outputs.json)
echo "INSTANCE_ID=$INSTANCE_ID"
export HCACHE_IP=$(jq -r ".$STACK_NAME | map(select(.OutputKey == \"hcache_ip\"))[0].OutputValue?" stack.outputs.json)
aliyun ecs ModifyInstanceVncPasswd --InstanceId=$INSTANCE_ID --VncPassword=Hca123
ssh-keygen -f "~/.ssh/known_hosts" -R "$ECS_IP" || true
echo "export HCACHE_HOST=http://$HCACHE_IP:8080"
