#!/bin/bash
namespace=cort0
host_interface="cort0-host"
bridge_interface="cort0"

sudo ip link del $bridge_interface
sudo ip link del $host_interface
sudo ip netns del $namespace