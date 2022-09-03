#!/bin/bash
set -xe
export IMAGE_ID="$(jq -r '.ImageId' image.latest.json)"
if [ -z $IMAGE_ID ]; then
    echo "No image id is found!"
    exit
fi
ros-cdk synth
jq -r ".Parameters.ecs_image_id.Default = \"$IMAGE_ID\"" submission.template.json | tee application.ros.json
zip ${IMAGE_ID}.zip application.ros.json
cp ${IMAGE_ID}.zip latest.zip