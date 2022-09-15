use std::sync::Arc;
use actix_web::{get, HttpResponse, routes, web};
use serde::Deserialize;
use tokio::sync::mpsc::{Receiver, Sender};
use crate::project_processor::{ProjectRequest};
use crate::repository::Repository;

#[derive(Debug, Deserialize)]
pub struct Request {
    pub repository: String,
    pub project: String,
    pub version: String,
    pub file: Option<String>,
}

#[routes]
#[get("/{repository}/{project}/{version}/{file:.*}")]
#[get("/{repository}/{project}/{version}/")]
pub async fn get_javadoc(requests: web::Data<Sender<ProjectRequest>>, request: web::Path<Request>, repositories: web::Data<Vec<Arc<Repository>>>) -> actix_web::Result<HttpResponse> {
    let repository: &Arc<Repository> = repositories.iter().find(|repository| repository.name == request.repository).ok_or(actix_web::error::ErrorNotFound("Repository not found"))?;

    Ok(HttpResponse::Ok().body("OK"))
}