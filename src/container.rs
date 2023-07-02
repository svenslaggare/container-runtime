use std::ffi::{c_int, c_void, CString};
use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use log::{error, info, trace};

use crate::helpers::RemoveDirGuard;
use crate::linux::{exec, mount, waitpid, wrap_libc_error};
use crate::model::{ContainerRuntimeError, ContainerRuntimeResult, User};
use crate::network::NetworkNamespace;
use crate::spec::{DNSSpec, NetworkSpec, RunContainerSpec};

pub fn run(run_container_spec: &RunContainerSpec) -> ContainerRuntimeResult<()> {
    let mut child_stack = vec![0u8; 32 * 1024];

    let _remove_container_root = RemoveDirGuard::new(run_container_spec.container_root());
    let network_namespace = if let NetworkSpec::Bridged(bridged) = &run_container_spec.network {
        Some(NetworkNamespace::create(run_container_spec.network_namespace().unwrap(), bridged)?)
    } else {
        None
    };

    let pid = unsafe {
        extern "C" fn clone_callback(args: *mut c_void) -> c_int {
            let args = args as *const RunContainerSpec;
            if let Err(err) = execute(unsafe { &*args }) {
                error!("Container execute failed due to: {}", err.to_string());
                -1
            } else {
                0
            }
        }

        let clone_network_namespace = if network_namespace.is_some() {libc::CLONE_NEWNET} else {0};

        wrap_libc_error(libc::clone(
            clone_callback,
            child_stack.as_mut_ptr().offset(child_stack.len() as isize) as *mut c_void,
            libc::CLONE_NEWPID | libc::CLONE_NEWNS | libc::CLONE_NEWUTS | clone_network_namespace | libc::SIGCHLD,
            run_container_spec as *const _ as *mut c_void
        ))
    }?;

    info!("Running container as PID {}.", pid);
    let status = waitpid(pid)?;
    info!("PID {} exited with status {}.", pid, status);

    Ok(())
}

fn execute(spec: &RunContainerSpec) -> ContainerRuntimeResult<()> {
    setup_cpu_cgroup(&spec.id, spec.cpu_shares)?;
    setup_memory_cgroup(&spec.id, spec.memory, spec.memory_swap)?;

    if let Some(network_namespace) = spec.network_namespace() {
        setup_network(&network_namespace, spec.hostname())?;
    }

    mount(None, Path::new("/"), None, libc::MS_PRIVATE | libc::MS_REC, None)?;

    let new_root = create_container_root(&spec.image_root(), &spec.container_root())?;
    info!("Container root: {}", new_root.to_str().unwrap());

    setup_dns(&new_root, &spec.dns)?;

    let users = User::from_passwd_file(&new_root.join("etc").join("passwd"))?;
    let user = match spec.user(users.values()) {
        Some(user) => Some(user?),
        None => None
    };

    let working_dir = user
        .as_ref()
        .map(|user| user.home_folder.clone())
        .unwrap_or(Path::new("/").to_owned());

    setup_container_root(&new_root, &working_dir, &spec.bind_mounts)?;

    if let Some(user) = user.as_ref() {
        setup_user(user)?;
    }

    exec(&spec.command)?;

    Ok(())
}

fn create_container_root(image_root: &Path, container_root: &Path) -> ContainerRuntimeResult<PathBuf> {
    trace!("Create container root - image root: {}, container root: {}", image_root.to_str().unwrap(), container_root.to_str().unwrap());

    let container_cow_rw = container_root.join("cow_rw");
    let container_cow_workdir = container_root.join("cow_workdir");
    let container_rootfs = container_root.join("rootfs");

    for path in [&container_cow_rw, &container_cow_workdir, &container_rootfs] {
        if !path.exists() {
            std::fs::create_dir_all(path)?;
        }
    }

    mount(
        Some("overlay"),
        &container_rootfs,
        Some("overlay"),
        libc::MS_NODEV,
        Some(&format!(
            "lowerdir={},upperdir={},workdir={}",
            image_root.to_str().unwrap(),
            container_cow_rw.to_str().unwrap(),
            container_cow_workdir.to_str().unwrap()
        ))
    )?;

    Ok(container_rootfs)
}

fn setup_container_root(new_root: &Path, working_dir: &Path, bind_mounts: &Vec<(PathBuf, PathBuf)>) -> ContainerRuntimeResult<()> {
    trace!("Setup container root - new root: {}, working dir: {}", new_root.to_str().unwrap(), working_dir.to_str().unwrap());

    let inner = || -> ContainerRuntimeResult<()> {
        setup_mounts(&new_root)?;
        setup_devices(&new_root)?;

        let old_root = new_root.join("old_root");
        std::fs::create_dir_all(&old_root)?;

        for (source, target) in bind_mounts {
            let target_in_new_root = new_root.join(target.iter().skip(1).collect::<PathBuf>());
            std::fs::create_dir_all(&target_in_new_root)?;
            mount(Some(source.to_str().unwrap()), &target_in_new_root, None, libc::MS_BIND, None)?;
        }

        unsafe {
            let new_root_str = CString::new(new_root.to_str().unwrap()).unwrap();
            let old_root_str = CString::new(old_root.to_str().unwrap()).unwrap();

            wrap_libc_error(libc::syscall(
                libc::SYS_pivot_root,
                new_root_str.as_ptr(),
                old_root_str.as_ptr()
            ) as i32)?;
        }

        unsafe {
            let working_dir = CString::new(working_dir.to_str().unwrap()).unwrap();
            wrap_libc_error(libc::chdir(working_dir.as_ptr()))?;
        }

        unsafe {
            let target = CString::new("/old_root").unwrap();
            wrap_libc_error(libc::umount2(target.as_ptr(), libc::MNT_DETACH))?;
        }

        std::fs::remove_dir("/old_root")?;

        Ok(())
    };

    inner().map_err(|err| ContainerRuntimeError::SetupContainerRoot(err.to_string()))
}

fn setup_cpu_cgroup(container_id: &str, cpu_shares: Option<i64>) -> ContainerRuntimeResult<()> {
    trace!("Setup cpu group - cpu shares: {:?}", cpu_shares);

    let inner = || -> ContainerRuntimeResult<()> {
        let container_cpu_cgroup_dir = create_cgroup_task(container_id, "cpu")?;

        if let Some(cpu_shares) = cpu_shares {
            std::fs::write(container_cpu_cgroup_dir.join("cpu.shares"), cpu_shares.to_string())?;
        }

        Ok(())
    };

    inner().map_err(|err| ContainerRuntimeError::SetupCpuCgroup(err.to_string()))
}

fn setup_memory_cgroup(container_id: &str, memory: Option<i64>, memory_swap: Option<i64>) -> ContainerRuntimeResult<()> {
    trace!("Setup memory group - memory: {:?}, memory_swap: {:?}", memory, memory_swap);

    let inner = || -> ContainerRuntimeResult<()> {
        let container_memory_cgroup_dir = create_cgroup_task(container_id, "memory")?;

        if let Some(memory) = memory {
            std::fs::write(container_memory_cgroup_dir.join("memory.limit_in_bytes"), memory.to_string())?;
        }

        if let Some(memory_swap) = memory_swap {
            std::fs::write(container_memory_cgroup_dir.join("memory.memsw.limit_in_bytes"), memory_swap.to_string())?;
        }

        Ok(())
    };

    inner().map_err(|err| ContainerRuntimeError::SetupMemoryCgroup(err.to_string()))
}

fn create_cgroup_task(container_id: &str, task_type: &str) -> ContainerRuntimeResult<PathBuf> {
    let container_cgroup_dir = Path::new(&format!("/sys/fs/cgroup/{}", task_type)).join("container_runtime").join(container_id);
    if !container_cgroup_dir.exists() {
        std::fs::create_dir_all(&container_cgroup_dir)?;
    }

    File::create(container_cgroup_dir.join("tasks"))?
        .write_all(std::process::id().to_string().as_bytes())?;

    Ok(container_cgroup_dir)
}

fn setup_network(network_namespace: &str, hostname: Option<String>) -> ContainerRuntimeResult<()> {
    trace!("Setup network - namespace: {}, hostname: {:?}", network_namespace, hostname);

    let inner = || -> ContainerRuntimeResult<()> {
        let file = File::open(format!("/run/netns/{}", network_namespace))?;
        unsafe {
            wrap_libc_error(libc::setns(file.as_raw_fd(), libc::CLONE_NEWNET))?;
        }

        if let Some(hostname) = hostname {
            unsafe {
                let hostname = CString::new(hostname).unwrap();
                wrap_libc_error(libc::sethostname(hostname.as_ptr(), hostname.as_bytes().len()))?;
            }
        }

        Ok(())
    };

    inner().map_err(|err| ContainerRuntimeError::SetupNetwork(err.to_string()))
}

fn setup_dns(new_root: &Path, spec: &DNSSpec) -> ContainerRuntimeResult<()> {
    let resolv_content = match spec {
        DNSSpec::Server(servers) => {
            servers
                .iter()
                .map(|server| format!("nameserver {}", server))
                .collect::<Vec<_>>()
                .join("\n") + "\n"
        }
        DNSSpec::CopyFromHost => std::fs::read_to_string("/etc/resolv.conf")?
    };

    trace!("Setup DNS - content: {}", resolv_content.replace("\n", " "));

    let inner = || -> ContainerRuntimeResult<()> {
        std::fs::write(new_root.join("etc").join("resolv.conf"), resolv_content)?;
        Ok(())
    };

    inner().map_err(|err| ContainerRuntimeError::SetupDNS(err.to_string()))
}

fn setup_user(user: &User) -> ContainerRuntimeResult<()> {
    trace!("Setup user - user: {:?}", user);

    let inner = || -> ContainerRuntimeResult<()> {
        std::env::set_var("HOME", user.home_folder.to_str().unwrap());

        unsafe {
            if let Some(group_id) = user.group_id {
                wrap_libc_error(libc::setgid(group_id as libc::gid_t))?;
            }

            wrap_libc_error(libc::setuid(user.id as libc::uid_t))?;
        }
        Ok(())
    };

    inner().map_err(|err| ContainerRuntimeError::SetupUser(err.to_string()))
}

fn setup_mounts(new_root: &Path) -> ContainerRuntimeResult<()> {
    trace!("Setup mounts - new root: {}", new_root.to_str().unwrap());

    let inner = || -> ContainerRuntimeResult<()> {
        mount(Some("proc"), &new_root.join("proc"), Some("proc"), 0, None)?;
        mount(Some("sysfs"), &new_root.join("sys"), Some("sysfs"), 0, None)?;
        mount(Some("tmpfs"), &new_root.join("dev"), Some("tmpfs"), libc::MS_NOSUID | libc::MS_STRICTATIME, Some("mode=755"))?;

        let devpts_path = new_root.join("dev").join("pts");
        if !devpts_path.exists() {
            std::fs::create_dir_all(&devpts_path).unwrap();
            mount(Some("devpts"), &devpts_path, Some("devpts"), 0, None)?;
        }

        Ok(())
    };

    inner().map_err(|err| ContainerRuntimeError::SetupMounts(err.to_string()))
}

fn setup_devices(new_root: &Path) -> ContainerRuntimeResult<()> {
    let dev_path = new_root.join("dev");
    trace!("Setup devices - dev path: {}", dev_path.to_str().unwrap());

    let inner = || -> ContainerRuntimeResult<()> {
        for (i, dev) in ["stdin", "stdout", "stderr"].iter().enumerate() {
            std::os::unix::fs::symlink(&format!("/proc/self/fd/{}", i), dev_path.join(dev))?;
        }

        let devices = [
            ("null", (libc::S_IFCHR, 1, 3)),
            ("zero", (libc::S_IFCHR, 1, 5)),
            ("random", (libc::S_IFCHR, 1, 8)),
            ("urandom", (libc::S_IFCHR, 1, 9)),
            ("console", (libc::S_IFCHR, 136, 1)),
            ("tty", (libc::S_IFCHR, 5, 0)),
            ("full", (libc::S_IFCHR, 1, 7)),
        ];

        for (device, (device_type, major, minor)) in devices {
            unsafe {
                let pathname = CString::new(dev_path.join(device).to_str().unwrap()).unwrap();
                wrap_libc_error(libc::mknod(
                    pathname.as_ptr(),
                    0o666 | device_type,
                    libc::makedev(major, minor),
                ))?;
            }
        }

        Ok(())
    };

    inner().map_err(|err| ContainerRuntimeError::SetupDevices(err.to_string()))
}