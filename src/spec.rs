use std::path::{Path, PathBuf};

use crate::model::{ContainerRuntimeError, ContainerRuntimeResult, User};

#[derive(Debug, Clone)]
pub struct RunContainerSpec {
    pub image_base_dir: PathBuf,
    pub containers_base_dir: PathBuf,
    pub id: String,
    pub image: String,
    pub command: Vec<String>,
    pub network: NetworkSpec,
    pub user: Option<UserSpec>,
    pub cpu_shares: Option<i64>,
    pub memory: Option<i64>,
    pub memory_swap: Option<i64>
}

impl RunContainerSpec {
    pub fn image_root(&self) -> PathBuf {
        self.image_base_dir.join(&self.image).join("rootfs")
    }

    pub fn container_root(&self) -> PathBuf {
        self.containers_base_dir.join(&self.id)
    }

    pub fn hostname(&self) -> Option<String> {
        match &self.network {
            NetworkSpec::Host => None,
            NetworkSpec::Bridged(bridged) => {
                Some(bridged.hostname.clone().unwrap_or_else(|| self.id.clone()))
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
pub enum NetworkSpec {
    Host,
    Bridged(BridgedNetworkSpec)
}

#[derive(Debug, Clone)]
pub struct BridgedNetworkSpec {
    pub bridge_interface: String,
    pub bridge_ip_address: String,
    pub container_ip_address: String,
    pub hostname: Option<String>
}