use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::model::{ContainerRuntimeError, ContainerRuntimeResult, User};
use crate::network;
use crate::network::Ipv4Net;

#[derive(Debug, Clone)]
pub struct RunContainerSpec {
    pub image_base_dir: PathBuf,
    pub containers_base_dir: PathBuf,
    pub id: String,
    pub name: String,
    pub image: String,
    pub command: Vec<String>,
    pub network: NetworkSpec,
    pub dns: DNSSpec,
    pub user: Option<UserSpec>,
    pub cpu_shares: Option<i64>,
    pub memory: Option<i64>,
    pub memory_swap: Option<i64>,
    pub bind_mounts: Vec<BindMountSpec>
}

impl RunContainerSpec {
    pub fn image_root(&self) -> PathBuf {
        self.image_base_dir.join("rootfs").join(&self.image)
    }

    pub fn image_archive(&self) -> PathBuf {
        self.image_base_dir.join(self.image.clone() + ".tar")
    }

    pub fn container_root(&self) -> PathBuf {
        self.containers_base_dir.join(&self.id)
    }

    pub fn hostname(&self) -> Option<String> {
        match &self.network {
            NetworkSpec::Host => None,
            NetworkSpec::Bridged(bridged) => {
                Some(bridged.hostname.clone().unwrap_or_else(|| self.name.clone()))
            }
        }
    }

    pub fn user<'a, T: Iterator<Item=&'a User>>(&'a self, users: T) -> Option<ContainerRuntimeResult<User>> {
        let user = self.user.as_ref()?;

        Some(
            user
                .find_user(users)
                .ok_or_else(|| ContainerRuntimeError::InvalidUser(user.clone()))
        )
    }

    pub fn network_namespace(&self) -> Option<String> {
        match &self.network {
            NetworkSpec::Host => None,
            NetworkSpec::Bridged(_) => Some(format!("cort-{}", &self.id[..4]))
        }
    }
}

#[derive(Debug, Clone)]
pub enum UserSpec {
    Name(String),
    Id(i32),
    IdAndGroupId(i32, i32)
}

impl UserSpec {
    pub fn find_user<'a, T: Iterator<Item=&'a User>>(&'a self, users: T) -> Option<User> {
        match self {
            UserSpec::Name(name) => {
                for user in users {
                    if &user.username == name {
                        return Some(user.clone());
                    }
                }

                None
            }
            UserSpec::Id(id) => {
                for user in users {
                    if &user.id == id {
                        return Some(user.clone());
                    }
                }

                Some(
                    User {
                        username: "unknown".to_string(),
                        id: *id,
                        group_id: None,
                        home_folder: Path::new("/root").to_owned()
                    }
                )
            }
            UserSpec::IdAndGroupId(user_id, group_id) => {
                for user in users {
                    if &user.id == user_id && &user.group_id == &Some(*group_id) {
                        return Some(user.clone());
                    }
                }

                Some(
                    User {
                        username: "unknown".to_string(),
                        id: *user_id,
                        group_id: Some(*group_id),
                        home_folder: Path::new("/root").to_owned()
                    }
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct BridgeSpec {
    pub physical_interface: Option<String>,
    pub interface: String,
    pub ip_address: Ipv4Net
}

impl BridgeSpec {
    pub fn get_default() -> ContainerRuntimeResult<BridgeSpec> {
        Ok(
            BridgeSpec {
                physical_interface: Some(network::find_internet_interface()?),
                interface: "cort0".to_string(),
                ip_address: Ipv4Net::from_str("10.10.1.1/16").unwrap()
            }
        )
    }
}

#[derive(Debug, Clone)]
pub enum NetworkSpec {
    Host,
    Bridged(BridgedNetworkSpec)
}

impl NetworkSpec {
    pub fn is_host(&self) -> bool {
        match self {
            NetworkSpec::Host => true,
            _ => false
        }
    }

    pub fn default_dns(&self) -> DNSSpec {
        if self.is_host() {
            DNSSpec::CopyFromHost
        } else {
            DNSSpec::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct BridgedNetworkSpec {
    pub bridge_interface: String,
    pub bridge_ip_address: Ipv4Net,
    pub container_ip_address: Ipv4Net,
    pub hostname: Option<String>
}

impl BridgedNetworkSpec {
    pub fn from_bridge(bridge: &BridgeSpec) -> ContainerRuntimeResult<BridgedNetworkSpec> {
        Ok(
            BridgedNetworkSpec {
                bridge_interface: bridge.interface.clone(),
                bridge_ip_address: bridge.ip_address.clone(),
                container_ip_address: network::find_free_ip_address(bridge.ip_address)?,
                hostname: None
            }
        )
    }

    pub fn with_hostname(mut self, hostname: Option<String>) -> BridgedNetworkSpec {
        self.hostname = hostname;
        self
    }
}

#[derive(Debug, Clone)]
pub enum DNSSpec {
    Server(Vec<String>),
    CopyFromHost
}

impl Default for DNSSpec {
    fn default() -> Self {
        DNSSpec::Server(vec!["8.8.8.8".to_owned(), "8.8.4.4".to_owned()])
    }
}

#[derive(Debug, Clone)]
pub struct BindMountSpec {
    pub source: PathBuf,
    pub target: PathBuf,
    pub is_readonly: bool
}