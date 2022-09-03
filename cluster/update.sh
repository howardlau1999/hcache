#!/bin/bash

for i in $(seq 1 3); do
  echo "Updating node $i"
  curl -X POST -H "Content-Type: application/json" -d "{\"hosts\": [\"172.16.0.153\", \"172.16.0.157\", \"172.16.0.145\"], \"index\": $i}" http://cache-$((i-1)):8080/updateCluster
done