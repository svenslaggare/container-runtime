#!/bin/bash
set -eo pipefail
docker build -t ubuntu-container-runtime -f sample.dockerfile .

rm -rf images/ubuntu
mkdir -p images/ubuntu/rootfs
docker export $(docker create ubuntu-container-runtime) --output=images/ubuntu.tar
cd images
tar -xf ubuntu.tar -C ubuntu/rootfs
rm -f ubuntu.tar