#!/usr/bin/env bash
export INSTANCE_ID=$(jq -r '.TestStack | map(select(.OutputKey == "instance_id"))[0].OutputValue?' stack.outputs.json)
aliyun ecs StopInstance --InstanceId $INSTANCE_ID