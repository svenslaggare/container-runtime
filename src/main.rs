use std::path::{ PathBuf};
use std::str::FromStr;

use log::{error, LevelFilter};
use uuid::Uuid;
use structopt::StructOpt;

mod model;
mod spec;
mod container;
mod network;
mod linux;
mod helpers;

use crate::spec::{BindMountSpec, BridgedNetworkSpec, BridgeSpec, NetworkSpec, RunContainerSpec, UserSpec};
use crate::model::{ContainerRuntimeResult};

fn main() {
    let console_config: ConsoleConfig = ConsoleConfig::from_args();
    if let Err(err) = run(console_config) {
        error!("Failure: {}", err.to_string());
        std::process::exit(1);
    }
}

fn run(console_config: ConsoleConfig) -> ContainerRuntimeResult<()> {
    setup_logging(&console_config).unwrap();

    let base_dir = std::env::current_dir().unwrap();
    let image_base_dir = base_dir.join("images");
    let containers_base_dir = base_dir.join("containers");

    let network = match console_config.network {
        Network::Host => {
            NetworkSpec::Host
        }
        Network::Bridge => {
            let bridge = BridgeSpec::default()?;
            network::create_bridge(&bridge)?;

            let bridged = BridgedNetworkSpec::from_bridge(&bridge)?
                .with_hostname(console_config.hostname);

            NetworkSpec::Bridged(bridged)
        }
    };

    let id = Uuid::new_v4().to_string();
    let dns = network.default_dns();

    let run_container_spec = RunContainerSpec {
        image_base_dir,
        containers_base_dir,
        id: id.clone(),
        name: console_config.name.unwrap_or_else(|| id),
        image: console_config.image,
        command: console_config.command,
        network,
        dns,
        user: console_config.user.map(|user| UserSpec::Name(user)),
        cpu_shares: Some(256),
        memory: Some(1024 * 1024 * 1024),
        memory_swap: None,
        bind_mounts: BindMountSpec::from_paths(console_config.mounts)?
    };

    container::run(&run_container_spec)
}

#[derive(Debug, StructOpt)]
#[structopt(name="cort", about="Container runtime")]
struct ConsoleConfig {
    /// The log level
    #[structopt(long)]
    log_level: Option<LevelFilter>,
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
    /// The paths to bind mount into the container
    #[structopt(long)]
    mounts: Vec<PathBuf>,
    /// The image to run
    #[structopt()]
    image: String,
    /// The command to run
    #[structopt()]
    command: Vec<String>
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

fn setup_logging(console_config: &ConsoleConfig) -> Result<(), log::SetLoggerError> {
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
        .level(console_config.log_level.unwrap_or(LevelFilter::Debug))
        .chain(std::io::stdout())
        .apply()?;
    Ok(())
}