#!/bin/bash
set -eo pipefail

namespace=$1

if [ $# -ne 1 ]; then
    echo "Expected 1 arguments."
    exit 1
fi

umount /run/netns/$namespace
rm -rf /run/netns/$namespace