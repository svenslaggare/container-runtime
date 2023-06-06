use std::fmt::{Display};
use std::net::Ipv4Addr;
use std::process::Command;
use std::str::FromStr;

use log::error;

use crate::model::{ContainerRuntimeError, ContainerRuntimeResult};
use crate::spec::{BridgedNetworkSpec, BridgeNetworkSpec};

pub fn create_bridge(bridge: &BridgeNetworkSpec) -> ContainerRuntimeResult<()> {
    let result = Command::new("bash")
        .args(["scripts/try_create_bridge.sh", &bridge.physical_interface, &bridge.interface, &bridge.ip_address.to_string()])
        .spawn().unwrap()
        .wait().unwrap();

    if !result.success() {
        return Err(ContainerRuntimeError::CreateNetworkBridge);
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
        .args([
            "scripts/create_network_namespace.sh",
            &bridge.bridge_interface,
            &bridge.bridge_ip_address.to_string(),
            network_namespace,
            &bridge.container_ip_address.to_string()
        ])
        .spawn().unwrap()
        .wait().unwrap();

    if !result.success() {
        return Err(ContainerRuntimeError::CreateNetworkNamespace);
    }

    Ok(())
}

fn destroy_network_namespace(network_namespace: &str) -> ContainerRuntimeResult<()> {
    let result = Command::new("bash")
        .args(["scripts/destroy_network_namespace.sh", &network_namespace])
        .spawn().unwrap()
        .wait().unwrap();

    if !result.success() {
        return Err(ContainerRuntimeError::CreateNetworkNamespace);
    }

    Ok(())
}

pub fn find_free_ip_address(base_ip_address: Ipv4Net) -> Option<Ipv4Net> {
    let network_namespaces = find_container_network_namespaces();
    let check_is_ip_address_used = |ip_address: Ipv4Net| {
        if is_ip_address_used(&ip_address, None) {
            return true;
        }

        for namespace in &network_namespaces {
            if is_ip_address_used(&ip_address, Some(namespace)) {
                return true;
            }
        }

        false
    };

    let mut next_ip_address = base_ip_address;
    for _ in 0..base_ip_address.subnet_size() {
        if !next_ip_address.is_broadcast() && !next_ip_address.is_network() {
            if !check_is_ip_address_used(next_ip_address) {
                return Some(next_ip_address);
            }
        }

        next_ip_address = next_ip_address.next();
    }

    None
}

fn find_container_network_namespaces() -> Vec<String> {
    let result = Command::new("ip")
        .args(["netns", "list"])
        .output().unwrap();

    let output = String::from_utf8(result.stdout).unwrap();
    output
        .lines()
        .map(|line| line.split(" ").next().unwrap().to_owned())
        .filter(|namespace| namespace.starts_with("cort-"))
        .collect()
}

fn is_ip_address_used(ip_address: &Ipv4Net, namespace: Option<&str>) -> bool {
    let arguments = if let Some(namespace) = namespace {
        vec!["netns", "exec", namespace, "ip", "addr", "show"]
    } else {
        vec!["addr", "show"]
    };

    let result = Command::new("ip")
        .args(arguments)
        .output().unwrap();

    let output = String::from_utf8(result.stdout).unwrap();
    output.contains(&ip_address.to_string())
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ipv4Net {
    pub address: Ipv4Addr,
    pub subnet_cidr: u16
}

impl Ipv4Net {
    pub fn new(address: Ipv4Addr, subnet_cidr: u16) -> Ipv4Net {
        Ipv4Net {
            address,
            subnet_cidr
        }
    }

    pub fn subnet_mask(&self) -> u32 {
        !((1 << (32 - self.subnet_cidr) as u32) - 1)
    }

    pub fn subnet_size(&self) -> u32 {
        (32 - self.subnet_cidr) as u32
    }

    pub fn next(&self) -> Ipv4Net {
        let (network_part, host_part) = self.split();

        let next_address = network_part | ((host_part + 1) & !self.subnet_mask());
        let next_address = Ipv4Addr::from(next_address);

        Ipv4Net::new(next_address, self.subnet_cidr)
    }

    pub fn is_network(&self) -> bool {
        let (_, host_part) = self.split();
        host_part == 0
    }

    pub fn is_broadcast(&self) -> bool {
        let (_, host_part) = self.split();
        host_part == (1 << (32 - self.subnet_cidr)) - 1
    }

    fn split(&self) -> (u32, u32) {
        let addr_uint = u32::from_be_bytes(self.address.octets());
        let subnet_mask = self.subnet_mask();

        let network_part = addr_uint & subnet_mask;
        let host_part = addr_uint & !subnet_mask;
        (network_part, host_part)
    }
}

impl Display for Ipv4Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.address, self.subnet_cidr)
    }
}

impl FromStr for Ipv4Net {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.split("/");
        let address = parts.next().ok_or_else(|| "Expected IP address.")?;
        let subnet_size = parts.next().ok_or_else(|| "Expected cidr notation.")?;

        let address = Ipv4Addr::from_str(address).map_err(|err| format!("Failed to parse IP address: {}", err))?;
        let subnet_cidr = u16::from_str(subnet_size).map_err(|err| format!("Failed to parse subnet mask: {}", err))?;

        Ok(Ipv4Net::new(address, subnet_cidr))
    }
}

#[test]
fn test_ipv4net_from_str() {
    assert_eq!(Ok(Ipv4Net::new(Ipv4Addr::new(127, 0, 0, 1), 17)), Ipv4Net::from_str("127.0.0.1/17"));
}

#[test]
fn test_ipv4net_subnet_mask() {
    let net1 = Ipv4Net::new(Ipv4Addr::new(127, 0, 0, 1), 24);
    let net2 = Ipv4Net::new(Ipv4Addr::new(127, 0, 0, 1), 17);
    assert_eq!([255, 255, 255, 0], net1.subnet_mask().to_be_bytes());
    assert_eq!([255, 255, 128, 0], net2.subnet_mask().to_be_bytes());
}

#[test]
fn test_ipv4net_next_address() {
    let net1 = Ipv4Net::new(Ipv4Addr::new(127, 41, 12, 1), 24);

    let mut current = net1;
    for _ in 0..255 {
        current = current.next();
    }

    assert_eq!(Ipv4Net::new(Ipv4Addr::new(127, 41, 12, 0), 24), current);
    assert_eq!(true, current.is_network());
}