#!/bin/bash
export IMAGE_ID="$(jq -r '.ImageId' image.latest.json)"
if [ -z $IMAGE_ID ]; then
    echo "No image id is found!"
    exit
fi
echo -n "Deleting $IMAGE_ID! Confirm: "
read REPLY
if [ "$REPLY" != "y" ] && [ "$REPLY" != "Y" ]; then
    exit
fi

set -xe
aliyun ecs ModifyImageSharePermission --ImageId="$IMAGE_ID" --RegionId="cn-beijing" --RemoveAccount.1="1828723137315221"
export SNAPSHOT_ID="$(aliyun ecs DescribeImages --ImageId=$IMAGE_ID | jq -r '.Images.Image[0].DiskDeviceMappings.DiskDeviceMapping[].SnapshotId')"
aliyun ecs DeleteImage --ImageId="$IMAGE_ID"
aliyun ecs DeleteSnapshot --SnapshotId="$SNAPSHOT_ID"

rm image.latest.json
