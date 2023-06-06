#!/bin/bash
set -eo pipefail

namespace=$1

if [ $# -ne 1 ]; then
    echo "Expected 1 arguments."
    exit 1
fi

ip netns del $namespace
ip link del $namespace-host
