use std::str::FromStr;

use log::error;
use uuid::Uuid;
use structopt::StructOpt;

mod model;
mod spec;
mod container;
mod network;
mod linux;
mod helpers;

use crate::network::Ipv4Net;
use crate::spec::{BridgedNetworkSpec, BridgeNetworkSpec, DNSSpec, NetworkSpec, RunContainerSpec, UserSpec};
use crate::model::ContainerRuntimeResult;

fn main() {
    let console_config: ConsoleConfig = ConsoleConfig::from_args();
    if let Err(err) = run(console_config) {
        error!("Failure: {}", err.to_string());
        std::process::exit(1);
    }
}

fn run(console_config: ConsoleConfig) -> ContainerRuntimeResult<()> {
    setup_logging().unwrap();

    let base_dir = std::env::current_dir().unwrap();
    let image_base_dir = base_dir.join("images");
    let containers_base_dir = base_dir.join("containers");

    let network = match console_config.network {
        Network::Host => {
            NetworkSpec::Host
        }
        Network::Bridge => {
            let bridge = BridgeNetworkSpec {
                physical_interface: Some(network::find_internet_interface()?),
                interface: "cort0".to_string(),
                ip_address: Ipv4Net::from_str("10.10.1.1/16").unwrap()
            };

            network::create_bridge(&bridge)?;
            let mut bridged = BridgedNetworkSpec::from_bridge(&bridge)?;
            bridged.hostname = console_config.hostname;

            NetworkSpec::Bridged(bridged)
        }
    };

    let dns = if network.is_host() { DNSSpec::CopyFromHost } else {DNSSpec::default()};

    let run_container_spec = RunContainerSpec {
        image_base_dir,
        containers_base_dir,
        id: console_config.name.unwrap_or_else(|| Uuid::new_v4().to_string()),
        image: console_config.image,
        command: console_config.command,
        network,
        dns,
        user: console_config.user.map(|user| UserSpec::Name(user)),
        cpu_shares: Some(256),
        memory: Some(1024 * 1024 * 1024),
        memory_swap: None
    };

    container::run(&run_container_spec)?;
    Ok(())
}

#[derive(Debug, StructOpt)]
#[structopt(name="cort", about="Container runtime")]
struct ConsoleConfig {
    /// The name of the container
    #[structopt(long)]
    name: Option<String>,
    /// The user to use
    #[structopt(short, long)]
    user: Option<String>,
    /// The network type to use
    #[structopt(long="net", default_value="bridge")]
    network: Network,
    /// The hostname to use
    #[structopt(long)]
    hostname: Option<String>,
    /// The image to run
    #[structopt()]
    image: String,
    /// The command to run
    #[structopt()]
    command: Vec<String>,
}

#[derive(Debug)]
enum Network {
    Host,
    Bridge
}

impl FromStr for Network {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "host" => Ok(Network::Host),
            "bridge" => Ok(Network::Bridge),
            _ => Err("Invalid network mode.".to_owned())
        }
    }
}

fn setup_logging() -> Result<(), log::SetLoggerError> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S.%f]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Debug)
        .chain(std::io::stdout())
        .apply()?;
    Ok(())
}