#!/bin/bash
set -eo pipefail

pid=$1
namespace=$2

if [ $# -ne 2 ]; then
    echo "Expected 2 arguments."
    exit 1
fi

mkdir -p /run/netns/
touch /run/netns/$namespace
mount --bind /proc/$pid/ns/net /run/netns/$namespace