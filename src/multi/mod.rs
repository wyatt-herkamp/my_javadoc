use std::sync::Arc;

use actix_web::web::ServiceConfig;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use tokio::sync::mpsc::Sender;

use crate::project_processor::ProjectRequest;
use crate::repository::Repository;

#[derive(Debug, Deserialize)]
pub struct Request {
    pub repository: String,
    pub project: String,
    pub version: String,
    pub file: Option<String>,
}

pub fn register_web(service: &mut ServiceConfig) {
    service.service(
        web::resource([
            "/{repository}/{project}/{version}/{file:.*}",
            "/{repository}/{project}/{version}/",
        ])
        .name("get_javadoc")
        .route(web::get().to(get_javadoc)),
    );
}

pub async fn get_javadoc(
    requests: web::Data<Sender<ProjectRequest>>,
    request: web::Path<Request>,
    repositories: web::Data<Vec<Arc<Repository>>>,
) -> actix_web::Result<HttpResponse> {
    let repository: Arc<Repository> = repositories
        .iter()
        .find(|repository| repository.name == request.repository)
        .ok_or(actix_web::error::ErrorNotFound("Repository not found"))?
        .clone();
    let request = request.into_inner();
    if let Some(project) = repository.get_project(&request.project).await? {
        let (text, version) = if request.version.eq("latest") {
            if let Some(v) = project.latest.as_ref() {
                if let Some(x) = project.versions.get(v) {
                    (v, x)
                } else {
                    requests
                        .send(ProjectRequest {
                            repository,
                            project: request.project,
                            version: Some(v.to_owned()),
                        })
                        .await
                        .map_err(|_| {
                            actix_web::error::ErrorInternalServerError("Failed to send request")
                        })?;
                    return Err(actix_web::error::ErrorNotFound("Version not found"));
                }
            } else {
                return Err(actix_web::error::ErrorNotFound("No latest version found"));
            }
        } else {
            if let Some(v) = project.versions.get(&request.version) {
                (&request.version, v)
            } else {
                requests
                    .send(ProjectRequest {
                        repository,
                        project: request.project,
                        version: Some(request.version),
                    })
                    .await
                    .map_err(|_| {
                        actix_web::error::ErrorInternalServerError("Failed to send request")
                    })?;

                return Err(actix_web::error::ErrorNotFound("Version not found"));
            }
        };
        if version.should_be_sent_for_rebuilding(&repository) {
            requests
                .send(ProjectRequest {
                    repository,
                    project: request.project,
                    version: Some(text.clone()),
                })
                .await
                .map_err(|_| {
                    actix_web::error::ErrorInternalServerError("Failed to send request")
                })?;
        }
        let option = version.load_file(request.file).await?;
        if let Some(file) = option {
            return Ok(HttpResponse::Ok()
                .content_type(file.content_type)
                .body(file.file));
        } else {
            return Err(actix_web::error::ErrorNotFound("File not found"));
        }
    } else {
        requests
            .send(ProjectRequest {
                repository,
                project: request.project,
                version: None,
            })
            .await
            .map_err(|_| actix_web::error::ErrorInternalServerError("Failed to send request"))?;
        return Err(actix_web::error::ErrorNotFound("Project not found"));
    }
}
