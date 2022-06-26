#!/bin/bash
set -xe
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

aliyun ecs ModifyImageSharePermission --ImageId="$IMAGE_ID" --RegionId="cn-beijing" --RemoveAccount.1="1828723137315221"

aliyun ecs DeleteImage --ImageId="$IMAGE_ID"

rm image.latest.json
