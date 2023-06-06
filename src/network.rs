use std::net::Ipv4Addr;
use std::process::Command;

use log::error;

use crate::model::{ContainerRuntimeError, ContainerRuntimeResult};
use crate::spec::BridgedNetworkSpec;

pub fn create_bridge(physical_interface: &str, bridge: &BridgedNetworkSpec) -> ContainerRuntimeResult<()> {
    let result = Command::new("bash")
        .args(["try_create_bridge.sh", physical_interface, &bridge.bridge_interface, &bridge.bridge_ip_address])
        .spawn().unwrap()
        .wait().unwrap();

    if !result.success() {
        return Err(ContainerRuntimeError::FailedToCreateNetworkBridge);
    }

    Ok(())
}

pub struct NetworkNamespace {
    name: String
}

impl NetworkNamespace {
    pub fn create(name: String, network: &BridgedNetworkSpec) -> ContainerRuntimeResult<NetworkNamespace> {
        create_network_namespace(network, &name)?;

        Ok(
            NetworkNamespace {
                name
            }
        )
    }
}

impl Drop for NetworkNamespace {
    fn drop(&mut self) {
        if let Err(err) = destroy_network_namespace(&self.name) {
            error!("Failed to destroy network namespace: {}", err);
        }
    }
}

fn create_network_namespace(bridge: &BridgedNetworkSpec, network_namespace: &str) -> ContainerRuntimeResult<()> {
    let result = Command::new("bash")
        .args(["create_network_namespace.sh", &bridge.bridge_interface, &bridge.bridge_ip_address, network_namespace, &bridge.container_ip_address])
        .spawn().unwrap()
        .wait().unwrap();

    if !result.success() {
        return Err(ContainerRuntimeError::FailedToCreateNetworkNamespace);
    }

    Ok(())
}

fn destroy_network_namespace(network_namespace: &str) -> ContainerRuntimeResult<()> {
    let result = Command::new("bash")
        .args(["destroy_network_namespace.sh", &network_namespace])
        .spawn().unwrap()
        .wait().unwrap();

    if !result.success() {
        return Err(ContainerRuntimeError::FailedToCreateNetworkNamespace);
    }

    Ok(())
}

pub fn find_free_ip_address(base_ip_address: Ipv4Addr) -> Option<Ipv4Addr> {
    let mut next_ip_address_parts = base_ip_address.octets();
    for _ in 0..1024 {
        let next_ip_address = Ipv4Addr::new(next_ip_address_parts[0], next_ip_address_parts[1], next_ip_address_parts[2], next_ip_address_parts[3]);
        if !is_ip_address_used(&next_ip_address.to_string()) {
            return Some(next_ip_address);
        }

        next_ip_address_parts[3] += 1;
        if next_ip_address_parts[3] == 0 {
            next_ip_address_parts[2] += 1;
        }
    }

    None
}

pub fn is_ip_address_used(ip_address: &str) -> bool {
    let result = Command::new("ip")
        .args(["addr", "show"])
        .output().unwrap();

    let output = String::from_utf8(result.stdout).unwrap();
    output.contains(ip_address)
}