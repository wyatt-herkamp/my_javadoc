use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;

use crate::project::Project;
use crate::{ConfigRepository, Error};

#[derive(Debug)]
pub struct Repository {
    pub name: String,
    pub address: String,
    pub allows_redeploy: bool,
    pub path: PathBuf,
}

impl Repository {
    pub fn new(name: String, config: ConfigRepository, path: impl AsRef<Path>) -> Self {
        let address = if config.address.ends_with("/") {
            config.address.trim_end_matches("/").to_string()
        } else {
            config.address
        };
        Repository {
            path: path.as_ref().join(&name),
            name,
            address: address,
            allows_redeploy: config.allows_redeploy,
        }
    }
    /// Returns the Project if it exists
    pub async fn get_project(&self, project: impl AsRef<str>) -> Result<Option<Project>, Error> {
        let project_cache = self.path.join(project_to_path(project.as_ref()));
        if !project_cache.exists() {
            return Ok(None);
        }
        let project_file = project_cache.join("project.json");
        if !project_file.exists() {
            Ok(None)
        } else {
            let mut file = std::fs::File::open(project_file)?;
            let project: Project = serde_json::from_reader(&mut file)?;
            Ok(Some(project))
        }
    }
    pub async fn save_project(&self, project: Project) -> Result<(), Error> {
        let project_cache = self.path.join(project_to_path(project.name.as_str()));
        if !project_cache.exists() {
            tokio::fs::create_dir_all(&project_cache).await?;
        }
        let project_file = project_cache.join("project.json");
        let mut file = tokio::fs::File::create(project_file).await?;
        let project = serde_json::to_string_pretty(&project)?;
        file.write_all(project.as_bytes()).await?;
        Ok(())
    }
}

#[inline(always)]
pub fn project_to_path(project: impl AsRef<str>) -> String {
    project.as_ref().replace(".", "/").replace(":", "/")
}
