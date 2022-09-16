use actix_web::{get, HttpResponse};
use actix_web::web::Data;
use handlebars::Handlebars;
use serde_json::json;

#[get("/")]
pub async fn index(data: Data<Handlebars<'_>>) -> HttpResponse {
    let body = data.render("site/index.html", &json!({})).unwrap();
    HttpResponse::Ok().body(body)
}