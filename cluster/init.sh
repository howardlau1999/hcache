#!/bin/bash

for i in $(seq 1 3); do
  echo "Updating node $i"
  curl http://cache-$((i-1)):8080/init
done