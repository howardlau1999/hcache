#!/usr/bin/env bash
STACK_NAME=${STACK_NAME:-TestStack}
export INSTANCE_ID=$(jq -r ".${STACK_NAME} | map(select(.OutputKey == \"instance_id\"))[0].OutputValue?" stack.outputs.json)
aliyun ecs StopInstance --InstanceId $INSTANCE_ID
