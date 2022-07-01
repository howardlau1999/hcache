#!/usr/bin/env bash
set -e
echo -n "Destroy benchmark ECS stack! Confirm: "
read REPLY
if [ "$REPLY" != "y" ] && [ "$REPLY" != "Y" ]; then
    exit
fi
ros-cdk destroy BenchmarkStack --sync=true --quiet=true
rm stack.outputs.json
if [ -f test.stacks.output.json ]; then
    echo "Found test.stacks.output.json, restoring from test.stacks.output.json"
    mv test.stacks.output.json stacks.output.json
fi
