#!/bin/bash

BINARY_PATH=${BINARY_PATH:-/usr/bin/hcache}
ansible all -m ansible.builtin.copy -a "src=${BINARY_PATH} dest=/usr/bin/hcache mode=0755" -f 3
