use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use thiserror::Error;

use crate::spec::UserSpec;

#[derive(Error, Debug)]
pub enum ContainerRuntimeError {
    #[error("Failed to create network bridge: {0}")]
    CreateNetworkBridge(String),
    #[error("Failed to create network namespace: {0}")]
    CreateNetworkNamespace(String),
    #[error("Failed to destroy network namespace: {0}")]
    DestroyNetworkNamespace(String),

    #[error("Failed to setup cpu cgroup: {0}")]
    SetupCpuCgroup(String),
    #[error("Failed to setup memory cgroup: {0}")]
    SetupMemoryCgroup(String),
    #[error("Failed to setup network stack: {0}")]
    SetupNetwork(String),
    #[error("Failed to setup DNS: {0}")]
    SetupDNS(String),
    #[error("Failed to setup user: {0}")]
    SetupUser(String),
    #[error("Failed to setup container root: {0}")]
    SetupContainerRoot(String),
    #[error("Failed to setup mounts: {0}")]
    SetupMounts(String),
    #[error("Failed to setup devices: {0}")]
    SetupDevices(String),

    #[error("User not found: {0:?}")]
    InvalidUser(UserSpec),
    #[error("No free IP address found in network")]
    NetworkIsFull,
    #[error("Failed to determine internet interface: {0}")]
    FailedToDetermineInternetInterface(String),

    #[error("IP command failure: {0}")]
    IPCommand(String),
    #[error("IPTables command failure: {0}")]
    IPTablesCommand(String),
    #[error("Failed to mount: {0}")]
    Mount(String),
    #[error("Failed to execute: {0}")]
    Execute(String),

    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Libc error: {0}")]
    Libc(String)
}

pub type ContainerRuntimeResult<T> = Result<T, ContainerRuntimeError>;

#[derive(Debug, Clone)]
pub struct User {
    pub username: String,
    pub id: i32,
    pub group_id: Option<i32>,
    pub home_folder: PathBuf
}

impl User {
    pub fn from_passwd_file(passwd_path: &Path) -> ContainerRuntimeResult<HashMap<i32, User>> {
        let mut users = HashMap::new();

        if let Ok(mut file) = File::open(passwd_path) {
            let mut content = String::new();
            file.read_to_string(&mut content)?;

            for line in content.lines() {
                let parts = line.split(":").collect::<Vec<_>>();

                if parts.len() >= 6 {
                    let username = parts[0].to_owned();
                    let user_id = i32::from_str(parts[2]).unwrap();
                    let group_id = i32::from_str(parts[3]).unwrap();
                    let home_folder = Path::new(parts[5]).to_owned();

                    users.insert(
                        user_id,
                        User {
                            username,
                            id: user_id,
                            group_id: Some(group_id),
                            home_folder
                        }
                    );
                }
            }
        }

        Ok(users)
    }
}