use crate::models::{Admin, Image, Site, Tag, project::{Langauge, Project}};
use askama::Template;
use askama_web::WebTemplate;

#[derive(Template, WebTemplate)]
#[template(path = "project.html")]
pub struct ProjectTemplate {
    pub project: Project,
    pub language: Langauge,
    pub tags: Vec<Tag>,
    pub is_admin: bool,
    pub site: Site,
    pub description: String,
}

#[derive(Template, WebTemplate)]
#[template(path = "project_form.html")]
pub struct ProjectFormTemplate {
    pub project: Project,
    pub language: Langauge,
    pub tags: Vec<(Tag, bool, i64)>,
    pub images: Vec<Image>,
    pub action: String,
    pub site: Site,
}

#[derive(Template, WebTemplate)]
#[template(path = "partials/tag-checkbox.html")]
pub struct TagCheckbox {
    pub tag: Tag,
    pub checked: bool,
    pub used_by_others: i64,
}

#[derive(Template, WebTemplate)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub site: Site,
    pub error: Option<String>,
}

#[derive(Template, WebTemplate)]
#[template(path = "projects.html")]
pub struct ProjectsTemplate {
    pub admin: Option<Admin>,
    pub projects: Vec<Project>,
    pub tags: Vec<Tag>,
    pub site: Site,
    pub tag: Option<String>,
    pub page: i64,
    pub total_pages: i64,
}

impl ProjectsTemplate {
    pub fn page_url(&self, page: i64) -> String {
        match &self.tag {
            Some(tag) => format!("/projects?page={page}&tag={}", urlencoding::encode(tag)),
            None => format!("/projects?page={page}"),
        }
    }
}

#[derive(Template, WebTemplate)]
#[template(path = "partials/admin-project.html")]
pub struct AdminProjectRow {
    pub project: Project,
}

#[derive(Template, WebTemplate)]
#[template(path = "partials/image.html")]
pub struct ImageItem {
    pub image: Image,
}

#[derive(Template, WebTemplate)]
#[template(path = "admin.html")]
pub struct AdminTemplate {
    pub site: Site,
}

#[derive(Template, WebTemplate)]
#[template(path = "not_found.html")]
pub struct NotFoundTemplate {
    pub site: Site,
}
