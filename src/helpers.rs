use std::path::{PathBuf};
use log::error;

pub struct RemoveDirGuard {
    dir: PathBuf
}

impl RemoveDirGuard {
    pub fn new(dir: PathBuf) -> RemoveDirGuard {
        RemoveDirGuard {
            dir
        }
    }
}

impl Drop for RemoveDirGuard {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.dir) {
            error!("Failed to remove directory {} due to: {}", self.dir.to_str().unwrap(), err);
        }
    }
}