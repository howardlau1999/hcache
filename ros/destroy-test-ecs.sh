#!/usr/bin/env bash
set -xe
echo -n "Destroy test ECS stack! Confirm: "
read REPLY
if [ "$REPLY" != "y" ] && [ "$REPLY" != "Y" ]; then
    exit
fi
ros-cdk destroy TestStack --sync=true --quiet=true
rm stack.outputs.json