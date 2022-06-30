#!/usr/bin/env bash
set -xe
export INSTANCE_ID=$(jq -r '.TestStack | map(select(.OutputKey == "instance_id"))[0].OutputValue?' stack.outputs.json)
echo "INSTANCE_ID=$INSTANCE_ID"
export VNC_URL=$(aliyun ecs DescribeInstanceVncUrl --InstanceId=$INSTANCE_ID | jq -r '.VncUrl')
echo "https://g.alicdn.com/aliyun/ecs-console-vnc2/0.0.8/index.html?password=Hca123&isWindows=false&instanceId=$INSTANCE_ID&vncUrl=$VNC_URL"
