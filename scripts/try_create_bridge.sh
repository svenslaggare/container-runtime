#!/bin/bash
set -eo pipefail

host_phy_interface=$1

bridge_interface=$2
bridge_ip_address=$3

if [ $# -ne 3 ]; then
    echo "Expected 3 arguments."
    exit 1
fi

if ! ip link show $bridge_interface &> /dev/null; then
    ./scripts/create_bridge.sh "$host_phy_interface" "$bridge_interface" "$bridge_ip_address"
fi