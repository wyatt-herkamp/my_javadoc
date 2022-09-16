use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use log::debug;
use maven_rs::maven_metadata::DeployMetadata;
use maven_rs::quick_xml;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::fs::{read, read_to_string};
use tokio::io::AsyncWriteExt;

use crate::repository::{project_to_path, Repository};
use crate::Error;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Project {
    pub name: String,
    pub versions: HashMap<String, Version>,
    pub latest: Option<String>,
    pub last_updated: Option<DateTime<Utc>>,
}

impl Project {
    /// Returns the latest version of the project
    pub fn get_latest_version(&self) -> Option<&Version> {
        if let Some(latest) = self.latest.as_ref() {
            // Should always be Some
            self.versions.get(latest)
        } else {
            None
        }
    }
    pub fn get_latest_version_mut(&mut self) -> Option<&mut Version> {
        if let Some(latest) = self.latest.as_ref() {
            // Should always be Some
            self.versions.get_mut(latest)
        } else {
            None
        }
    }
    /// Rather or not should we update the project
    pub fn should_update(&self, _repository: impl AsRef<Repository>) -> bool {
        if let Some(last_updated) = self.last_updated {
            let now = Utc::now();
            if (now - last_updated).num_hours() > 24 {
                return true;
            }
        }
        true
    }

    pub async fn download_deploy_data(
        &self,
        repository: impl AsRef<Repository>,
        client: &Client,
    ) -> Result<DeployMetadata, Error> {
        let project_path = project_to_path(self.name.as_str());
        let url = format!(
            "{}/{}/maven-metadata.xml",
            repository.as_ref().address,
            project_path
        );
        let response = client.get(&url).send().await?;
        let response = response.error_for_status()?;
        let text = response.text().await?;
        let metadata: DeployMetadata = quick_xml::de::from_str(text.as_str())?;
        let folder = repository.as_ref().path.join(project_path);
        if !folder.exists() {
            tokio::fs::create_dir_all(&folder).await?;
        }
        let mut file = tokio::fs::File::create(folder.join("maven-metadata.xml")).await?;
        file.write_all(text.as_bytes()).await?;
        Ok(metadata)
    }

    #[inline(always)]
    pub async fn get_deploy_data(
        &self,
        repository: impl AsRef<Repository>,
    ) -> Result<DeployMetadata, Error> {
        let project_path = project_to_path(self.name.as_str());

        let reader = read_to_string(
            repository
                .as_ref()
                .path
                .join(project_path)
                .join("maven-metadata.xml"),
        )
        .await?;
        quick_xml::de::from_str(reader.as_str()).map_err(Error::from)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Version {
    /// No Version Data
    NoBuild { checked: DateTime<Utc> },
    /// Contains a release version
    Build {
        path: PathBuf,
        sha1: Option<String>,
        built: DateTime<Utc>,
    },
    /// Contains a snapshot version
    BuildSnapshot {
        path: PathBuf,
        timestamp: DateTime<Utc>,
        built: DateTime<Utc>,
    },
}

pub struct FileResponse {
    pub file: Vec<u8>,
    pub content_type: String,
}

impl Version {
    /// Updates the timestamp of the version
    pub fn update_checked(&mut self, now: DateTime<Utc>) {
        match self {
            Version::NoBuild { checked } => *checked = now,
            Version::Build { built, .. } => *built = now,
            Version::BuildSnapshot { built, .. } => *built = now,
        }
    }

    pub async fn load_file(&self, file: Option<String>) -> Result<Option<FileResponse>, Error> {
        let result = match self {
            Version::NoBuild { .. } => {
                return Ok(None);
            }
            Version::Build { path, .. } => path,
            Version::BuildSnapshot { path, .. } => path,
        };
        let x = file
            .as_ref()
            .and_then(|f| if f.is_empty() { None } else { Some(f.as_str()) })
            .unwrap_or("index.html");
        let file = result.join(x);

        debug!("Loading file: {:?}", file);
        // TODO check if HTML page and add header
        if file.exists() {
            let text = read(file).await?;
            Ok(Some(FileResponse {
                file: text,
                content_type: mime_guess::from_path(x).first_or_octet_stream().to_string(),
            }))
        } else {
            Ok(None)
        }
    }
    /// Should the system check for updates
    pub fn should_be_sent_for_rebuilding(&self, _repo: impl AsRef<Repository>) -> bool {
        let now = Utc::now();

        match self {
            Version::NoBuild { checked } => {
                let difference = *checked - now;
                difference.num_hours() >= 24
            }
            Version::Build { built, .. } => {
                //TODO take the repository's update policy into account
                let difference = *built - now;
                difference.num_hours() >= 24
            }
            Version::BuildSnapshot { built, .. } => {
                let difference = *built - now;
                difference.num_hours() >= 24
            }
        }
    }
}
