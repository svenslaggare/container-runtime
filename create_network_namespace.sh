#!/bin/bash
set -eo pipefail

bridge_interface=$1
bridge_ip_address=$2

namespace=$3
namespace_ip_address=$4

if [ $# -ne 4 ]; then
    echo "Expected 4 arguments."
    exit 1
fi

ip netns add $namespace
./setup_network_namespace.sh "$bridge_interface" "$bridge_ip_address" "$namespace" "$namespace_ip_address"
echo "Created network namespace '$namespace' at $namespace_ip_address"