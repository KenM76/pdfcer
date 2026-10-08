//! A temp folder deleted when dropped, for tests that copy the binary: a
//! 56 MB copy left behind per run fills the disk.

use std::path::PathBuf;

pub struct Scratch(pub PathBuf);

impl std::ops::Deref for Scratch {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
