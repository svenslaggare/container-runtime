#!/bin/bash
set -eo pipefail

host_phy_interface=$1

bridge_ip_address=$2
bridge_interface=$3

if [ $# -ne 3 ]; then
    echo "Expected 3 arguments."
    exit 1
fi

ip link add name $bridge_interface type bridge
ip link set dev $bridge_interface up
ip addr add $bridge_ip_address dev $bridge_interface

echo 1 | tee -a /proc/sys/net/ipv4/ip_forward
iptables -P FORWARD DROP
iptables -F FORWARD
iptables -t nat -F
iptables -t nat -A POSTROUTING -s $bridge_ip_address -o $host_phy_interface -j MASQUERADE
iptables -A FORWARD -i $host_phy_interface -o $bridge_interface -j ACCEPT
iptables -A FORWARD -o $host_phy_interface -i $bridge_interface -j ACCEPT