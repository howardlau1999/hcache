#!/usr/bin/env bash
set -ex
source ./stop-ecs.sh
echo "Wait for ECS to stop..."
sleep 15
export TS="$(date --rfc-3339='seconds' | tr ' +' '_')"
export GIT_LOG="$(git log --format='%D %h %s' -n 1)"
export UNIX_TS="$(date +%s)"
aliyun ecs CreateImage --RegionId="cn-beijing" --Description="$TS $GIT_LOG" --InstanceId="$INSTANCE_ID" --ImageName="hcache-$TS" --Tag.1.Key="hcache" --Tag.1.Value="$UNIX_TS" | tee image.latest.json
export IMAGE_ID="$(jq -r '.ImageId' image.latest.json)"
aliyun ecs ModifyImageSharePermission --ImageId="$IMAGE_ID" --RegionId="cn-beijing" --AddAccount.1="1828723137315221" || echo "Image is still creating, try again a few minutes later"
# Wait for image creation
while true; do
    sleep 5
    export IMAGE_OBJECT=$(aliyun ecs DescribeImages --Status Available,Waiting,Creating --ImageId=$IMAGE_ID)
    export IMAGE_STATUS=$(echo "$IMAGE_OBJECT" | jq -r '.Images.Image[0].Status')
    export IMAGE_PROGRESS=$(echo "$IMAGE_OBJECT" | jq -r '.Images.Image[0].Progress')
    if [ "$IMAGE_STATUS" = "Available" ]; then
	break
    fi
    echo "$(date --rfc-3339='seconds') - $IMAGE_ID - $IMAGE_STATUS - $IMAGE_PROGRESS"
done
echo "Image created: $IMAGE_ID"
