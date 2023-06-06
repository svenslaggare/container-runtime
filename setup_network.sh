#!/bin/bash
set -eo pipefail

host_phy_interface="enp3s0"

bridge_interface="cort0"
bridge_ip_address="10.10.10.40/24"

namespace="cort0"
namespace_ip_address="10.10.10.10/24"

sudo ./create_bridge.sh "$host_phy_interface" "$bridge_interface" "$bridge_ip_address"
echo "Created bridge '$bridge_interface'"
sudo ip netns add $namespace
sudo ./setup_network_namespace.sh "$bridge_interface" "$bridge_ip_address" "$namespace" "$namespace_ip_address"
echo "Setup network namespace '$namespace'"