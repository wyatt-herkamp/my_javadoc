use std::path::{Path, PathBuf};
use crate::ConfigRepository;

#[derive(Debug)]
pub struct Repository {
    pub name: String,
    pub address: String,
    pub allows_redeploy: bool,
    pub path: PathBuf,
}

impl Repository {
    pub fn new(name: String, config: ConfigRepository, path: impl AsRef<Path>) -> Self {
        Repository {
            path: path.as_ref().join(&name),
            name,
            address: config.address,
            allows_redeploy: config.allows_redeploy,
        }
    }
}