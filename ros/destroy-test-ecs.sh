#!/usr/bin/env bash
set -x
ros-cdk destroy TestStack --sync=true --quiet=true
rm stack.outputs.json