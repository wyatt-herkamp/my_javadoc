use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use futures_util::StreamExt;
use log::{error, info};
use maven_rs::maven_metadata::DeployMetadata;
use maven_rs::quick_xml::de;
use maven_rs::snapshot_metadata::SnapshotMetadata;
use reqwest::{Client, ClientBuilder};
use tokio::fs::{remove_file, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::Receiver;

use crate::project::{Project, Version};
use crate::repository::Repository;
use crate::Error;

#[derive(Debug, Clone)]
pub struct ProjectRequest {
    pub repository: Arc<Repository>,
    /// The project id
    pub project: String,
    pub version: Option<String>,
}

pub async fn processor(cache: PathBuf, mut queue: Receiver<ProjectRequest>) {
    let client = ClientBuilder::new()
        .user_agent("My Javadoc Generator")
        .build()
        .unwrap();
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

pub async fn process_project(
    project_request: ProjectRequest,
    client: &Client,
) -> Result<(), Error> {
    info!("Processing project: {:?}", project_request);
    let project_path = project_to_path(&project_request.project);
    let project_location = project_request.repository.path.join(&project_path);

    let mut javadoc_project = project_request
        .repository
        .get_project(&project_request.project)
        .await?
        .unwrap_or_else(|| Project {
            name: project_request.project.clone(),
            versions: HashMap::new(),
            latest: None,
            last_updated: None,
        });
    let now = Utc::now();

    info!("Updating Deploy Data");
    javadoc_project.last_updated = Some(now.clone());
    // Download the latest maven-metadata.xml
    let deploy_data = javadoc_project
        .download_deploy_data(&project_request.repository, client)
        .await?;

    let (should_update, version_text) = if let Some(version_text) = project_request.version.as_ref()
    {
        if let Some(version) = javadoc_project.versions.get_mut(version_text) {
            match version {
                Version::NoBuild { checked } => {
                    if deploy_data
                        .versioning
                        .versions
                        .version
                        .contains(version_text)
                    {
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
        if version_text.ends_with("-SNAPSHOT") {
            let url = format!(
                "{}/{project_path}/{version_text}/maven-metadata.xml",
                project_request.repository.address
            );
            let response = client.get(&url).send().await?;
            if response.status().is_success() {
                let string = response.text().await?;
                let maven_file = project_location.join(version_text);
                if !maven_file.exists() {
                    tokio::fs::create_dir_all(&maven_file).await?;
                }
                let maven_file = maven_file.join("maven-metadata.xml");
                OpenOptions::new()
                    .write(true)
                    .create(true)
                    .open(&maven_file)
                    .await?
                    .write_all(string.as_bytes())
                    .await?;
                let metadata: SnapshotMetadata = de::from_str(&string)?;
                if let Some(javadoc_timestamp) = javadoc_project.versions.get(version_text) {
                    if let Version::BuildSnapshot { timestamp, .. } = javadoc_timestamp {
                        if let Some(snapshot) = metadata.versioning.snapshot {
                            if snapshot.timestamp.as_ref().eq(&Some(timestamp)) {
                                // The timestamp is the same, so we don't need to rebuild
                                return Ok(());
                            }
                        }
                    }
                }
                if let Some(value) = metadata.versioning.snapshot_versions {
                    let option = value.snapshot_version.into_iter().find(|x| {
                        if let Some(x) = x.classifier.as_ref() {
                            x.eq("javadoc")
                        } else {
                            false
                        }
                    });
                    if let Some(value) = option {
                        if build_javadoc(
                            &project_request,
                            client,
                            &project_location,
                            &deploy_data,
                            &version_text,
                            &value.value,
                        )
                        .await?
                        {
                            javadoc_project.versions.insert(
                                version_text.to_string(),
                                Version::BuildSnapshot {
                                    path: project_location.join(version_text),
                                    timestamp: value.updated.unwrap_or(now.clone()),
                                    built: now,
                                },
                            );
                        } else {
                            info!("Failed to build javadoc for snapshot");
                        }
                    }
                }
            }
        } else if build_javadoc(
            &project_request,
            client,
            &project_location,
            &deploy_data,
            version_text,
            version_text,
        )
        .await?
        {
            javadoc_project.versions.insert(
                version_text.to_string(),
                Version::Build {
                    path: project_location.join(version_text),
                    sha1: None,
                    built: now,
                },
            );
        } else {
            javadoc_project
                .versions
                .insert(version_text.to_string(), Version::NoBuild { checked: now });
        }
    }
    project_request
        .repository
        .save_project(javadoc_project)
        .await?;
    Ok(())
}

async fn build_javadoc(
    project_request: &ProjectRequest,
    client: &Client,
    project_location: &PathBuf,
    deploy_data: &DeployMetadata,
    version: impl AsRef<str>,
    version_text: &String,
) -> Result<bool, Error> {
    let project_path = project_to_path(&project_request.project);
    let url = format!(
        "{}/{project_path}/{version}/{}-{version_text}-javadoc.jar",
        project_request.repository.address,
        &deploy_data.artifact_id,
        version = version.as_ref()
    );
    let response = client.get(&url).send().await?;
    if response.status().is_success() {
        let download_jar = project_location.join(format!("{}.jar", version_text));
        if download_jar.exists() {
            remove_file(&download_jar).await?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .open(&download_jar)
            .await?;
        let mut stream = response.bytes_stream();

        while let Some(item) = stream.next().await {
            let chunk: Bytes = item?;
            file.write_all(chunk.as_ref()).await?;
        }
        let output_folder = project_location.join(version.as_ref());
        if output_folder.exists() && !output_folder.is_dir() {
            remove_file(&output_folder).await?;
        }
        if !output_folder.exists() {
            tokio::fs::create_dir(&output_folder).await?;
        }
        crate::zip::extract(&output_folder, &download_jar)?;
        Ok(true)
    } else {
        error!(
            "Failed to download javadoc for {project} {version}",
            project = project_request.project,
            version = version_text
        );
        Ok(false)
    }
}
