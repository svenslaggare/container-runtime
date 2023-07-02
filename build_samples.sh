#!/bin/bash
set -eo pipefail
docker build -t ubuntu-container-runtime -f sample.dockerfile .
docker export $(docker create ubuntu-container-runtime) --output=images/ubuntu.tar