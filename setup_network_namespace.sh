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

host_interface="$namespace-host"
namespace_interface="$namespace-ns"

ip link add $host_interface type veth peer name $namespace_interface
ip link set dev $host_interface master $bridge_interface
ip link set dev $namespace_interface master $bridge_interface

ip link set dev $host_interface up

ip link set $namespace_interface netns $namespace
ip netns exec $namespace ip addr add $namespace_ip_address dev $namespace_interface
ip netns exec $namespace ip link set dev $namespace_interface up
ip netns exec $namespace ip link set dev lo up
ip -n $namespace route add default via "${bridge_ip_address%/*}"