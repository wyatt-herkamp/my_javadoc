use std::collections::HashMap;
use std::io::BufReader;
use std::ops::Sub;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use log::{error, info};
use maven_rs::MavenFileExtension;
use reqwest::{Client, ClientBuilder};
use tokio::fs::{OpenOptions, remove_dir, remove_file};
use tokio::io::AsyncWriteExt;
use futures_util::StreamExt;
use tokio::sync::mpsc::Receiver;
use crate::Error;
use crate::project::{Project, Version};
use crate::repository::Repository;


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
        if let Err(error) = process_project(request, &client).await {
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

pub async fn process_project(project_request: ProjectRequest, client: &Client) -> Result<(), Error> {
    info!("Processing project: {:?}", project_request);
    let project_path = project_to_path(&project_request.project);
    let project_location = project_request.repository.path.join(&project_path);

    let mut javadoc_project = project_request.repository.get_project(&project_request.project).await?.unwrap_or_else(|| {
        Project {
            name: project_request.project.clone(),
            versions: HashMap::new(),
            latest: None,
            last_updated: None,
        }
    });
    let now = Utc::now();

    info!("Updating Deploy Data");
    javadoc_project.last_updated = Some(now.clone());
    // Download the latest maven-metadata.xml
    let deploy_data = javadoc_project.download_deploy_data(&project_request.repository, client).await?;


    let (should_update, version_text) = if let Some(version_text) = project_request.version.as_ref() {
        if let Some(version) = javadoc_project.versions.get_mut(version_text) {
            match version {
                Version::NoBuild { checked } => {
                    if deploy_data.versioning.versions.version.contains(version_text) {
                        (true, version_text)
                    } else {
                        // The version is not available
                        (false, version_text)
                    }
                }
                version => {
                    if version.should_be_sent_for_rebuilding(&project_request.repository) {
                        version.update_checked(now.clone());
                        (true, version_text)
                    } else {
                        (false, version_text)
                    }
                }
            }
        } else {
            // The version is not available
            (true, version_text)
        }
    } else {
        // This mean's it is requesting the latest version

        // Check if the latest version is the same as the latest version
        if javadoc_project.latest.as_ref() == deploy_data.get_latest_version() {
            if let Some(version) = javadoc_project.get_latest_version_mut() {
                if version.should_be_sent_for_rebuilding(&project_request.repository) {
                    version.update_checked(now.clone());
                    (true, javadoc_project.latest.as_ref().unwrap())
                } else {
                    (false, javadoc_project.latest.as_ref().unwrap())
                }
            } else {
                javadoc_project.latest = deploy_data.get_latest_version().cloned();
                // Update the latest version
                (true, javadoc_project.latest.as_ref().unwrap())
            }
        } else {
            javadoc_project.latest = deploy_data.get_latest_version().cloned();
            (true, javadoc_project.latest.as_ref().unwrap())
        }
    };

    if should_update {
        if version_text.ends_with("-SNAPSHOT") {} else {
            let url = format!("{}/{project_path}/{version_text}/{}-{version_text}-javadoc.jar", project_request.repository.address, &deploy_data.artifact_id);
            let response = client.get(&url).send().await?;
            if response.status().is_success() {
                let download_jar = project_location.join(format!("{}.jar", version_text));
                if download_jar.exists() {
                    remove_file(&download_jar).await?;
                }
                let mut file = OpenOptions::new().write(true).create(true).open(&download_jar).await?;
                let mut stream = response.bytes_stream();

                while let Some(item) = stream.next().await {
                    let chunk: Bytes = item?;
                    file.write_all(chunk.as_ref()).await?;
                }
                let output_folder = project_location.join(version_text);
                if output_folder.exists() && !output_folder.is_dir() {
                    remove_file(&output_folder).await?;
                }
                if !output_folder.exists() {
                    tokio::fs::create_dir(&output_folder).await?;
                }
                crate::zip::extract(&output_folder, &download_jar)?;
                javadoc_project.versions.insert(version_text.to_string(), Version::Build {
                    path: output_folder,
                    sha1: None,
                    built: now
                });


            } else {
                javadoc_project.versions.insert(version_text.to_string(), Version::NoBuild { checked: now });
                error!("Failed to download javadoc for {project} {version}", project = project_request.project, version = version_text);
            }
        }
    }
    project_request.repository.save_project(javadoc_project).await?;
    Ok(())
}


