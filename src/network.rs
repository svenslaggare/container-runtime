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