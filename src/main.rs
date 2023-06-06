use std::collections::HashMap;
use std::ffi::{c_int, c_ulong, c_void, CStr, CString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use thiserror::Error;

use uuid::Uuid;

fn main() {
    let id = Uuid::new_v4().to_string();
    let image_root = Path::new("/home/antjans/Code/container-runtime/images/ubuntu/rootfs");
    let container_root = Path::new("/home/antjans/Code/container-runtime/containers").join(&id);

    let run_container_spec = RunContainerSpec {
        id,
        image_root: image_root.to_owned(),
        container_root: container_root.to_owned(),
        command: vec!["/bin/bash".to_owned()],
        hostname: None,
        // user: Some(UserSpec::Name("ubuntu".to_owned())),
        user: None,
        cpu_shares: Some(256),
        memory: Some(1024 * 1024),
        memory_swap: None
    };

    let child_stack_size = 32 * 1024;
    let mut child_stack = vec![0u8; child_stack_size];

    let network_namespace = run_container_spec.network_namespace();
    create_network_namespace("cort0", "10.10.10.40/24", &network_namespace, "10.10.10.10/24").unwrap();

    let pid = unsafe {
        extern "C" fn clone_callback(args: *mut c_void) -> c_int {
            run_container(unsafe { &*(args as *const RunContainerSpec) }).unwrap();
            0
        }

        wrap_libc_error(libc::clone(
            clone_callback,
            child_stack.as_mut_ptr().offset(child_stack_size as isize) as *mut c_void,
            libc::CLONE_NEWPID | libc::CLONE_NEWNS | libc::CLONE_NEWUTS | libc::CLONE_NEWNET | libc::SIGCHLD,
            &run_container_spec as *const _ as *mut c_void
        )).unwrap()
    };

    let status = unsafe {
        let mut status = 0;
        wrap_libc_error(libc::waitpid(pid, &mut status as *mut c_int, 0)).unwrap();
        status
    };

    println!("PID {} exited status: {}", pid, status);
    std::fs::remove_dir_all(container_root).unwrap();
    destroy_network_namespace(&network_namespace).unwrap();
}

#[derive(Error, Debug)]
enum ContainerRuntimeError {
    #[error("Failed to create network namespace")]
    FailedToCreateNetworkNamespace,
    #[error("User not found: {0:?}")]
    InvalidUser(UserSpec),
    #[error("Failed to mount: {0}")]
    Mount(String),
    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Libc error: {0}")]
    Libc(String)
}

type ContainerRuntimeResult<T> = Result<T, ContainerRuntimeError>;

struct RunContainerSpec {
    id: String,
    image_root: PathBuf,
    container_root: PathBuf,
    command: Vec<String>,
    hostname: Option<String>,
    user: Option<UserSpec>,
    cpu_shares: Option<i64>,
    memory: Option<i64>,
    memory_swap: Option<i64>
}

impl RunContainerSpec {
    pub fn hostname(&self) -> String {
        self.hostname.clone().unwrap_or_else(|| self.id.clone())
    }

    pub fn user<'a, T: Iterator<Item=&'a User>>(&'a self, users: T) -> Option<ContainerRuntimeResult<User>> {
        let user = self.user.as_ref()?;

        Some(
            user
                .find_user(users)
                .ok_or_else(|| ContainerRuntimeError::InvalidUser(user.clone()))
        )
    }

    pub fn network_namespace(&self) -> String {
        format!("cort-{}", &self.id[..4])
    }
}

#[derive(Debug, Clone)]
enum UserSpec {
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

fn run_container(spec: &RunContainerSpec) -> ContainerRuntimeResult<()> {
    let file = File::open(format!("/run/netns/{}", spec.network_namespace()))?;
    unsafe {
        wrap_libc_error(libc::setns(
            file.as_raw_fd(),
            libc::CLONE_NEWNET
        )).unwrap();
    }

    setup_cpu_cgroup(&spec.id, spec.cpu_shares)?;
    setup_memory_cgroup(&spec.id, spec.memory, spec.memory_swap)?;

    unsafe {
        let hostname = CString::new(spec.hostname().as_str()).unwrap();
        wrap_libc_error(libc::sethostname(hostname.as_ptr(), hostname.as_bytes().len()))?;
    }

    mount(None, Path::new("/"), None, libc::MS_PRIVATE | libc::MS_REC, None)?;

    let new_root = create_container_root(&spec.image_root, &spec.container_root)?;
    println!("Container root: {}", new_root.to_str().unwrap());
    let users = User::from_passwd_file(&new_root.join("etc").join("passwd"))?;

    let mut dns_config = File::create(new_root.join("etc").join("resolv.conf"))?;
    dns_config.write_all(format!("nameserver 8.8.8.8").as_bytes())?;

    create_mounts(&new_root)?;

    let old_root = new_root.join("old_root");
    std::fs::create_dir_all(&old_root).unwrap();

    unsafe {
        let new_root_str = CString::new(new_root.to_str().unwrap()).unwrap();
        let old_root_str = CString::new(old_root.to_str().unwrap()).unwrap();

        wrap_libc_error(libc::syscall(
            libc::SYS_pivot_root,
            new_root_str.as_ptr(),
            old_root_str.as_ptr()
        ) as i32)?;
    }

    let user = match spec.user(users.values()) {
        Some(user) => Some(user?),
        None => None
    };

    let working_dir = user
        .as_ref()
        .map(|user| user.home_folder.clone())
        .unwrap_or(Path::new("/").to_owned());

    unsafe {
        let working_dir = CString::new(working_dir.to_str().unwrap()).unwrap();
        wrap_libc_error(libc::chdir(working_dir.as_ptr()))?;
    }

    if let Some(user) = user.as_ref() {
        std::env::set_var("HOME", user.home_folder.to_str().unwrap());
    }

    unsafe {
        let target = CString::new("/old_root").unwrap();
        wrap_libc_error(libc::umount2(target.as_ptr(), libc::MNT_DETACH))?;
    }

    std::fs::remove_dir("/old_root").unwrap();

    if let Some(user) = user.as_ref() {
        if let Some(group_id) = user.group_id{
            unsafe {
                wrap_libc_error(libc::setgid(group_id as libc::gid_t))?;
            }
        }

        unsafe {
            wrap_libc_error(libc::setuid(user.id as libc::gid_t))?;
        }
    }

    unsafe {
        let command = spec.command.iter().map(|part| CString::new(part.as_str()).unwrap()).collect::<Vec<_>>();
        let command_ptrs = command.iter().map(|part| part.as_ptr()).collect::<Vec<_>>();
        wrap_libc_error(libc::execvp(command_ptrs[0], &command_ptrs[0]))?;
    }

    Ok(())
}

fn create_container_root(image_root: &Path, container_root: &Path) -> ContainerRuntimeResult<PathBuf> {
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

fn setup_cpu_cgroup(container_id: &str, cpu_shares: Option<i64>) -> ContainerRuntimeResult<()> {
    let container_cpu_cgroup_dir = Path::new("/sys/fs/cgroup/cpu").join("container_runtime").join(container_id);
    if !container_cpu_cgroup_dir.exists() {
        std::fs::create_dir_all(&container_cpu_cgroup_dir)?;
    }

    File::create(container_cpu_cgroup_dir.join("tasks"))?
        .write_all(std::process::id().to_string().as_bytes())?;

    if let Some(cpu_shares) = cpu_shares {
        File::create(container_cpu_cgroup_dir.join("cpu.shares"))?
            .write_all(cpu_shares.to_string().as_bytes())?;
    }

    Ok(())
}

fn setup_memory_cgroup(container_id: &str, memory: Option<i64>, memory_swap: Option<i64>) -> ContainerRuntimeResult<()> {
    let container_memory_cgroup_dir = Path::new("/sys/fs/cgroup/memory").join("container_runtime").join(container_id);
    if !container_memory_cgroup_dir.exists() {
        std::fs::create_dir_all(&container_memory_cgroup_dir)?;
    }

    File::create(container_memory_cgroup_dir.join("tasks"))?
        .write_all(std::process::id().to_string().as_bytes())?;

    if let Some(memory) = memory {
        File::create(container_memory_cgroup_dir.join("memory.limit_in_bytes"))?
            .write_all(memory.to_string().as_bytes())?;
    }

    if let Some(memory_swap) = memory_swap {
        File::create(container_memory_cgroup_dir.join("memory.memsw.limit_in_bytes"))?
            .write_all(memory_swap.to_string().as_bytes())?
    }

    Ok(())
}

fn create_mounts(new_root: &Path) -> ContainerRuntimeResult<()> {
    mount(Some("proc"), &new_root.join("proc"), Some("proc"), 0, None)?;
    mount(Some("sysfs"), &new_root.join("sys"), Some("sysfs"), 0, None)?;
    mount(Some("tmpfs"), &new_root.join("dev"), Some("tmpfs"), libc::MS_NOSUID | libc::MS_STRICTATIME, Some("mode=755"))?;

    let devpts_path = new_root.join("dev").join("pts");
    if !devpts_path.exists() {
        std::fs::create_dir_all(&devpts_path).unwrap();
        mount(Some("devpts"), &devpts_path, Some("devpts"), 0, None)?;
    }

    create_devices(&new_root.join("dev"))?;
    Ok(())
}

fn create_devices(dev_path: &Path) -> ContainerRuntimeResult<()> {
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
                libc::makedev(major, minor)
            ))?;
        }
    }

    Ok(())
}

fn create_network_namespace(bridge_interface: &str, bridge_ip_address: &str, network_namespace: &str, namespace_ip_address: &str) -> ContainerRuntimeResult<()> {
    let result = Command::new("bash")
        .args(["create_network_namespace.sh", bridge_interface, bridge_ip_address, network_namespace, namespace_ip_address])
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

#[derive(Debug, Clone)]
struct User {
    username: String,
    id: i32,
    group_id: Option<i32>,
    home_folder: PathBuf
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

fn mount(src: Option<&str>, target: &Path, fstype: Option<&str>, flags: c_ulong, data: Option<&str>) -> ContainerRuntimeResult<()> {
    let src = src.map(|x| CString::new(x).unwrap());
    let target = CString::new(target.to_str().unwrap()).unwrap();
    let fstype = fstype.map(|x| CString::new(x).unwrap());
    let data = data.map(|x| CString::new(x).unwrap());

    unsafe {
        let result = libc::mount(
            src.as_ref().map(|x| x.as_ptr() as *const _).unwrap_or(std::ptr::null()),
            target.as_ptr() as *const _,
            fstype.as_ref().map(|x| x.as_ptr() as *const _).unwrap_or(std::ptr::null()),
            flags,
            data.as_ref().map(|x| x.as_ptr() as *const _).unwrap_or(std::ptr::null())
        );

        if result == 0 {
            Ok(())
        } else {
            Err(ContainerRuntimeError::Mount(extract_libc_error_message()))
        }
    }
}

fn wrap_libc_error(result: i32) -> ContainerRuntimeResult<i32> {
    if result >= 0 {
        Ok(result)
    } else {
        Err(ContainerRuntimeError::Libc(extract_libc_error_message()))
    }
}

fn extract_libc_error_message() -> String {
    unsafe {
        let error_message = CStr::from_ptr(libc::strerror(*libc::__errno_location()));
        error_message.to_str().unwrap().to_owned()
    }
}