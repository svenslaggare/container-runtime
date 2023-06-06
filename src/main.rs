use uuid::Uuid;

mod model;
mod spec;
mod container;
mod network;
mod linux;

use crate::spec::{BridgedNetworkSpec, NetworkSpec, RunContainerSpec, UserSpec};

fn main() {
    setup_logging().unwrap();

    let base_dir = std::env::current_dir().unwrap();
    let image_base_dir = base_dir.join("images");
    let containers_base_dir = base_dir.join("containers");

    let bridge = BridgedNetworkSpec {
        bridge_interface: "cort0".to_string(),
        bridge_ip_address: "10.10.10.1/24".to_string(),
        container_ip_address: "10.10.10.2/24".to_string(),
        hostname: None
    };

    network::create_bridge("enp3s0", &bridge).unwrap();

    let run_container_spec = RunContainerSpec {
        image_base_dir,
        containers_base_dir,
        id: Uuid::new_v4().to_string(),
        image: "ubuntu".to_string(),
        command: vec!["/bin/bash".to_owned()],
        network: NetworkSpec::Bridged(bridge),
        // network: NetworkSpec::Host,
        user: Some(UserSpec::Name("ubuntu".to_owned())),
        // user: None,
        cpu_shares: Some(256),
        memory: Some(1024 * 1024),
        memory_swap: None
    };

    container::run(&run_container_spec).unwrap();
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