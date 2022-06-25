#!/usr/bin/env bash
set -ex
source ./stop-ecs.sh
echo "Wait for ECS to stop..."
sleep 15
export TS="$(date --rfc-3339='seconds' | tr ' +' '_')"
export UNIX_TS="$(date +%s)"
aliyun ecs CreateImage --RegionId="cn-beijing" --Description="$TS" --InstanceId="$INSTANCE_ID" --ImageName="hcache-$TS" --Tag.1.Key="hcache" --Tag.1.Value="$UNIX_TS" | tee image.latest.json
export IMAGE_ID="$(jq -r '.ImageId' image.latest.json)"
aliyun ecs ModifyImageSharePermission --ImageId="$IMAGE_ID" --RegionId="cn-beijing" --AddAccount.1="1828723137315221" || echo "Image is still creating, try again a few minutes later"