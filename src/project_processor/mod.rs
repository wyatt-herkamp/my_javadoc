use std::collections::HashMap;
use std::io::BufReader;
use std::ops::Sub;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use chrono::{DateTime, Utc};
use log::{error, info};
use maven_rs::maven_metadata::DeployMetadata;
use maven_rs::quick_xml;
use maven_rs::quick_xml::DeError;
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs::{create_dir, read_to_string};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::Receiver;
use crate::{ConfigRepository};
use crate::repository::Repository;

#[derive(Debug, Error)]
pub enum ProjectProcessorError {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Maven(#[from] maven_rs::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    XMLError(#[from] DeError),
}


#[derive(Debug, Clone)]
pub struct ProjectRequest {
    pub repository: Arc<Repository>,
    /// The project id
    pub project: String,
    pub version: Option<String>,
}

pub async fn processor(cache: PathBuf, mut queue: Receiver<ProjectRequest>) {
    let client = ClientBuilder::new().user_agent("My Javadoc Generator").build().unwrap();
    while let Some(request) = queue.recv().await {
        if let Err(error) = process_project(request, &client, false).await {
            error!("Failed to process request {error}")
        }
    }
}

#[inline(always)]
pub fn project_to_path(project: &str) -> String {
    project.replace(".", "/").replace(":", "/")
}

#[inline(always)]
pub fn project_to_path_buf(project: &str) -> PathBuf {
    PathBuf::from(project_to_path(project))
}

pub async fn process_project(project_request: ProjectRequest, client: &Client, force: bool) -> Result<(), ProjectProcessorError> {
    info!("Processing project: {:?}", project_request);
    let project_location = project_request.repository.path.join(project_to_path(&project_request.project));

    let mut result = load_project(&project_request.repository.path, &project_request.project).await?;
    let now = Utc::now();
    let deploy_data: DeployMetadata = if let Some(last_updated) = result.last_updated.as_ref() {
        let difference = *last_updated - now;
        if difference.num_hours() >= 24 && !force {
            info!("Updating Deploy Data");
            result.last_updated = Some(now.clone());
            download_deploy_data(&project_location, &project_request, client).await?
        } else {
            get_deploy_data(&project_location, &project_request).await?
        }
    } else {
        info!("Updating Deploy Data");
        result.last_updated = Some(now.clone());
        download_deploy_data(&project_location, &project_request, client).await?
    };
    if let Some(version_text) = project_request.version {
        if let Some(version) = result.versions.get(&version_text) {
            match version {
                Version::NoBuild { checked } => {
                    if deploy_data.versioning.versions.version.contains(&version_text) {
                        // TODO download version
                    }
                }
                Version::Build { built, sha1 } => {
                    let difference = *built - now;
                    if difference.num_hours() >= 24 && !force {}
                }
                Version::BuildSnapshot { built, timestamp } => {
                    let difference = *built - now;
                    if difference.num_hours() >= 24 && !force {
                        // TODO re-download snapshot metadata then check for updates
                    }
                }
            }
        }
    }
    Ok(())
}

pub async fn download_deploy_data(project_location: impl AsRef<Path>, project: &ProjectRequest, client: &Client) -> Result<DeployMetadata, ProjectProcessorError> {
    let url = format!("{}/{}/maven-metadata.xml", project.repository.address, project_to_path(&project.project));
    let response = client.get(&url).send().await?;
    let response = response.error_for_status()?;
    let text = response.text().await?;
    let metadata: DeployMetadata = quick_xml::de::from_str(text.as_str())?;
    let mut file = tokio::fs::File::create(project_location.as_ref().join("maven-metadata.xml")).await?;
    file.write_all(text.as_bytes()).await?;
    Ok(metadata)
}

#[inline(always)]
pub async fn get_deploy_data(cache: impl AsRef<Path>, project: &ProjectRequest) -> Result<DeployMetadata, ProjectProcessorError> {
    let reader = read_to_string(cache.as_ref().join("maven-metadata.xml")).await?;
    quick_xml::de::from_str(reader.as_str()).map_err(ProjectProcessorError::from)
}

pub async fn load_project(cache: impl AsRef<Path>, project: impl AsRef<str>) -> Result<Project, ProjectProcessorError> {
    let cache = cache.as_ref();
    let project_cache = cache.join(project_to_path(project.as_ref()));
    info!("Loading project from cache: {:?}", project_cache);
    if !project_cache.exists() {
        create_dir(&project_cache).await?;
        return Ok(Project::default());
    }
    let project_file = project_cache.join("project.json");
    if !project_file.exists() {
        Ok(Project::default())
    } else {
        let mut file = std::fs::File::open(project_file)?;

        let project: Project = serde_json::from_reader(&mut file)?;
        Ok(project)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Project {
    pub versions: HashMap<String, Version>,

    pub last_updated: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Version {
    /// No Version Data
    NoBuild {
        checked: DateTime<Utc>,
    },
    /// Contains a release version
    Build {
        sha1: Option<String>,
        built: DateTime<Utc>,
    },
    /// Contains a snapshot version
    BuildSnapshot {
        timestamp: DateTime<Utc>,
        built: DateTime<Utc>,
    },
}