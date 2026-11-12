use askama::Template;
use askama_web::WebTemplate;

use crate::models::{Site, project::Project};

#[derive(Template, WebTemplate)]
#[template(path = "index.html")]
pub struct Index {
    pub projects: Vec<Project>,
    pub site: Site,
    pub description: Option<String>,
}
