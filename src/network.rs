use std::ffi::OsStr;
use std::fmt::{Display};
use std::net::{IpAddr, Ipv4Addr};
use std::process::Command;
use std::str::FromStr;

use log::{error, info};

use crate::model::{ContainerRuntimeError, ContainerRuntimeResult};
use crate::spec::{BridgedNetworkSpec, BridgeNetworkSpec};

pub fn create_bridge(bridge: &BridgeNetworkSpec) -> ContainerRuntimeResult<()> {
    if ip_command(["link", "show", &bridge.interface]).is_err() {
        let inner = || -> ContainerRuntimeResult<()> {
            ip_command(["link", "add", "name", &bridge.interface, "type", "bridge"])?;
            ip_command(["link", "set", "dev", &bridge.interface, "up"])?;
            ip_command(["addr", "add", &bridge.ip_address.to_string(), "dev", &bridge.interface])?;

            std::fs::write("/proc/sys/net/ipv4/ip_forward", "1")?;

            iptables_command(["-P", "FORWARD", "DROP"])?;
            iptables_command(["-F", "FORWARD"])?;
            iptables_command(["-A", "FORWARD", "-i", &bridge.interface, "-o", &bridge.interface, "-j", "ACCEPT"])?;

            if let Some(physical_interface) = &bridge.physical_interface {
                iptables_command(["-t", "nat", "-F"])?;
                iptables_command(["-t", "nat", "-A", "POSTROUTING", "-s", &bridge.ip_address.to_string(), "-o", physical_interface, "-j", "MASQUERADE"])?;
                iptables_command(["-A", "FORWARD", "-i", physical_interface, "-o", &bridge.interface, "-j", "ACCEPT"])?;
                iptables_command(["-A", "FORWARD", "-o", physical_interface, "-i", &bridge.interface, "-j", "ACCEPT"])?;
            }

            let physical_interface = bridge.physical_interface.clone().unwrap_or_else(|| "N/A".to_owned());
            info!("Created network bridge '{}' with IP {} using physical interface {}.", bridge.interface, bridge.ip_address, physical_interface);
            Ok(())
        };

        inner().map_err(|err| ContainerRuntimeError::CreateNetworkBridge(err.to_string()))
    } else {
        Ok(())
    }
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
            error!("Failed to destroy network namespace: {}", err.to_string());
        }
    }
}

fn create_network_namespace(bridge: &BridgedNetworkSpec, network_namespace: &str) -> ContainerRuntimeResult<()> {
    let inner = || -> ContainerRuntimeResult<()> {
        let host_interface = format!("{}-host", network_namespace);
        let namespace_interface = format!("{}-ns", network_namespace);

        ip_command(["netns", "add", network_namespace])?;

        ip_command(["link", "add", &host_interface, "type", "veth", "peer", "name", &namespace_interface])?;
        ip_command(["link", "set", "dev", &host_interface, "master", &bridge.bridge_interface])?;
        ip_command(["link", "set", "dev", &namespace_interface, "master", &bridge.bridge_interface])?;

        ip_command(["link", "set", "dev", &host_interface, "up"])?;

        ip_command(["link", "set", &namespace_interface, "netns", network_namespace])?;
        ip_command(["netns", "exec", network_namespace, "ip", "addr", "add", &bridge.container_ip_address.to_string(), "dev", &namespace_interface])?;
        ip_command(["netns", "exec", network_namespace, "ip", "link", "set", "dev", &namespace_interface, "up"])?;
        ip_command(["netns", "exec", network_namespace, "ip", "link", "set", "dev", "lo", "up"])?;
        ip_command(["-n", network_namespace, "route", "add", "default", "via", &bridge.bridge_ip_address.address.to_string()])?;
        Ok(())
    };

    inner().map_err(|err| ContainerRuntimeError::CreateNetworkNamespace(err.to_string()))
}

fn destroy_network_namespace(network_namespace: &str) -> ContainerRuntimeResult<()> {
    let inner = || -> ContainerRuntimeResult<()> {
        ip_command(["netns", "del", network_namespace])?;
        ip_command(["link", "del", &format!("{}-host", network_namespace)])?;
        Ok(())
    };

    inner().map_err(|err| ContainerRuntimeError::DestroyNetworkNamespace(err.to_string()))
}

pub fn find_free_ip_address(base_ip_address: Ipv4Net) -> ContainerRuntimeResult<Ipv4Net> {
    let network_namespaces = find_container_network_namespaces()?;
    let check_is_ip_address_used = |ip_address: Ipv4Net| -> ContainerRuntimeResult<bool> {
        if is_ip_address_used(&ip_address, None)? {
            return Ok(true);
        }

        for namespace in &network_namespaces {
            if is_ip_address_used(&ip_address, Some(namespace))? {
                return Ok(true);
            }
        }

        Ok(false)
    };

    let mut next_ip_address = base_ip_address;
    for _ in 0..base_ip_address.subnet_size() {
        if !next_ip_address.is_broadcast() && !next_ip_address.is_network() {
            if !check_is_ip_address_used(next_ip_address)? {
                return Ok(next_ip_address);
            }
        }

        next_ip_address = next_ip_address.next();
    }

    Err(ContainerRuntimeError::NetworkIsFull)
}

fn find_container_network_namespaces() -> ContainerRuntimeResult<Vec<String>> {
    Ok(
        ip_command(["netns", "list"])?
            .lines()
            .map(|line| line.split(" ").next().unwrap().to_owned())
            .filter(|namespace| namespace.starts_with("cort-"))
            .collect()
    )
}

fn is_ip_address_used(ip_address: &Ipv4Net, namespace: Option<&str>) -> ContainerRuntimeResult<bool> {
    let arguments = if let Some(namespace) = namespace {
        vec!["netns", "exec", namespace, "ip", "addr", "show"]
    } else {
        vec!["addr", "show"]
    };

    Ok(ip_command(arguments)?.contains(&ip_address.to_string()))
}

pub fn find_internet_interface() -> ContainerRuntimeResult<String> {
    let inner = || -> Result<String, String> {
        let hostname = "google.com";
        let ips: Vec<IpAddr> = dns_lookup::lookup_host(hostname).map_err(|err| err.to_string())?;

        for ip in ips {
            if let IpAddr::V4(ip) = ip {
                let result = ip_command(["route", "get", &ip.to_string()]).map_err(|err| err.to_string())?;
                let result = result.split(" ");
                let mut result = result.skip(4);
                return result.next().ok_or_else(|| "No interface found".to_owned()).map(|x| x.to_owned());
            }
        }

        Err("No IPv4 address found for host 'google.com'".to_owned())
    };

    inner().map_err(|err| ContainerRuntimeError::FailedToDetermineInternetInterface(err))
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

fn ip_command<I, S>(args: I) -> ContainerRuntimeResult<String> where I: IntoIterator<Item = S>, S: AsRef<OsStr> {
    let result = Command::new("ip")
        .args(args)
        .output()
        .unwrap();

    if !result.status.success() {
        return Err(ContainerRuntimeError::IPCommand(String::from_utf8(result.stderr).unwrap()));
    }

    Ok(String::from_utf8(result.stdout).unwrap())
}

fn iptables_command<I, S>(args: I) -> ContainerRuntimeResult<String> where I: IntoIterator<Item = S>, S: AsRef<OsStr> {
    let result = Command::new("iptables")
        .args(args)
        .output()
        .unwrap();

    if !result.status.success() {
        return Err(ContainerRuntimeError::IPTablesCommand(String::from_utf8(result.stderr).unwrap()));
    }

    Ok(String::from_utf8(result.stdout).unwrap())
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