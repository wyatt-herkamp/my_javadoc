use std::sync::Arc;
use actix_web::web::Data;
use log::info;
use tokio::sync::mpsc::Receiver;
use tux_lockfree::queue::Queue;
use crate::{ConfigRepository};

#[derive(Debug, Clone)]
pub struct Repository {
    pub name: String,
    pub address: String,
}

impl From<(String, ConfigRepository)> for Repository {
    fn from((name, data): (String, ConfigRepository)) -> Self {
        Self {
            name,
            address: data.address,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProjectRequest {
    pub repository: Arc<Repository>,
    /// The project id
    pub project: String,
    pub version: Option<String>,
}

pub async fn processor(mut queue: Receiver<ProjectRequest>) {
    while let Some(request) = queue.recv().await {
        info!("Processing request: {:?}", request);
    }
}

pub fn process_project(project_request: ProjectRequest) {
    info!("Processing project: {:?}", project_request);
}