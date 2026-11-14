use crate::{
    models::{
        Admin, COST, Image, NewImage, NewTag, Site, Tag, TagLink, meta_description, project::{InsertProject, Langauge, Project, ProjectChanges, render_markdown, slugify},
    }, rate_limit::{LoginLimiter, MAX_PER_IP, MAX_PER_USER}, schema::{
        admins::{self, dsl as a_dsl},
        images, project_tag_links, project_tags,
        projects::{self, dsl as p_dsl},
    }, templates::{
        index::Index, project::{
            AdminProjectRow, ImageItem, LoginTemplate, NotFoundTemplate, ProjectFormTemplate, ProjectTemplate, ProjectsTemplate, TagCheckbox,
        },
    },
};
use actix_files::NamedFile;
use actix_multipart::form::{MultipartForm, tempfile::TempFile, text::Text};
use actix_web::{
    App, Error, FromRequest, HttpRequest, HttpResponse, HttpServer,
    body::MessageBody,
    cookie::{Cookie, SameSite, time},
    dev::{ServiceRequest, ServiceResponse},
    error::{
        ErrorBadRequest, ErrorConflict, ErrorInternalServerError, ErrorNotFound,
        ErrorServiceUnavailable, InternalError,
    },
    get,
    http::{
        StatusCode,
        header::{
            CACHE_CONTROL, CONTENT_SECURITY_POLICY, ContentType, HeaderName, HeaderValue, LOCATION,
            REFERRER_POLICY, RETRY_AFTER, STRICT_TRANSPORT_SECURITY, X_CONTENT_TYPE_OPTIONS,
            X_FRAME_OPTIONS,
        },
    },
    middleware::{ErrorHandlerResponse, ErrorHandlers, Logger, Next, from_fn},
    post,
    web::{self, Form},
};
use anyhow::{Context, Result, anyhow, bail};
use askama::Template;
use diesel::{
    BoolExpressionMethods, Connection, ExpressionMethods, OptionalExtension, PgConnection, QueryDsl, QueryResult, RunQueryDsl, SelectableHelper, dsl::{insert_into, not, now}, r2d2::{ConnectionManager, Pool, PooledConnection}, result::{DatabaseErrorKind, Error as DieselError},
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use dotenvy::dotenv;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::from_str;
use std::{
    collections::HashMap,
    env,
    future::{Ready, ready},
    path::PathBuf,
    sync::RwLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub mod admin;
pub mod models;
pub mod rate_limit;
pub mod schema;
#[cfg(debug_assertions)]
pub mod seed;
pub mod templates;

pub const TOKEN: &'static str = "token";

pub type DbPool = Pool<ConnectionManager<PgConnection>>;
pub type DbConn = PooledConnection<ConnectionManager<PgConnection>>;

pub struct AppState {
    pub pool: DbPool,
    pub secret: String,
    pub upload_dir: PathBuf,
    pub site_url: String,
    pub login_limiter: LoginLimiter,
    pub dummy_hash: String,
    site: RwLock<Site>,
}

impl AppState {
    /// A connection from the pool. Waiting for one is blocking, so it happens off the async threads.
    pub async fn conn(&self) -> Result<DbConn, Error> {
        let pool = self.pool.clone();

        web::block(move || pool.get())
            .await
            .map_err(ErrorInternalServerError)?
            .map_err(|err| {
                error!("No database connection available: {err}");
                ErrorServiceUnavailable(err)
            })
    }

    pub fn cv_path(&self) -> PathBuf {
        self.upload_dir.join("cv.pdf")
    }

    pub fn site(&self) -> Site {
        self.site.read().unwrap().clone()
    }

    pub fn refresh_site(&self, conn: &mut PgConnection) -> QueryResult<()> {
        let site = Site {
            links: admin::load_links(conn)?,
            has_cv: self.cv_path().exists(),
        };

        *self.site.write().unwrap() = site;

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject
    pub sub: String,
    pub exp: usize,
    pub scope: String,
}

pub const SCOPE: &'static str = "projects:write";

pub fn issue_token(secret: &str, user: &str) -> Result<String> {
    let claims = Claims {
        sub: user.to_string(),
        exp: (SystemTime::now() + Duration::from_hours(12))
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize,
        scope: SCOPE.to_string(),
    };
    Ok(encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?)
}

fn redirect(to: &str) -> HttpResponse {
    HttpResponse::SeeOther()
        .insert_header((LOCATION, to))
        .finish()
}

fn to_login() -> Error {
    InternalError::from_response("Unauthorized", redirect("/login")).into()
}

/// Maps the diesel errors we care about to a proper status code
fn db_err(err: DieselError) -> Error {
    match err {
        DieselError::NotFound => ErrorNotFound("Not found"),
        DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, info) => {
            ErrorConflict(info.message().to_string())
        }
        err => {
            error!("Database error: {err}");
            ErrorInternalServerError(err)
        }
    }
}

fn html(body: String) -> HttpResponse {
    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(body)
}

/// Empty form fields arrive as "" rather than missing
fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty())
}

pub struct ProjectWriter(pub Claims);

impl FromRequest for ProjectWriter {
    type Error = actix_web::error::Error;

    type Future = Ready<std::result::Result<Self, Self::Error>>;

    fn from_request(req: &actix_web::HttpRequest, _: &mut actix_web::dev::Payload) -> Self::Future {
        let secret = req
            .app_data::<web::Data<AppState>>()
            .expect("State is missing")
            .secret
            .as_bytes();
        let token = req.cookie(TOKEN).map(|c| c.value().to_string());

        let Some(token) = token else {
            return ready(Err(to_login()));
        };

        let data = match decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret),
            &Validation::default(),
        ) {
            Ok(d) => d,
            Err(_) => return ready(Err(to_login())),
        };

        if !data.claims.scope.split(" ").any(|s| s == SCOPE) {
            return ready(Err(to_login()));
        }

        ready(Ok(ProjectWriter(data.claims)))
    }
}

fn project_by_slug(conn: &mut PgConnection, slug: &str) -> QueryResult<Project> {
    p_dsl::projects
        .filter(p_dsl::slug.eq(slug))
        .select(Project::as_select())
        .first(conn)
}

fn tags_of(conn: &mut PgConnection, project_id: i32) -> QueryResult<Vec<Tag>> {
    project_tag_links::table
        .inner_join(project_tags::table)
        .filter(project_tag_links::project_id.eq(project_id))
        .select(Tag::as_select())
        .order(project_tags::tag)
        .load(conn)
}

fn all_tags(conn: &mut PgConnection) -> QueryResult<Vec<Tag>> {
    project_tags::table
        .select(Tag::as_select())
        .order(project_tags::tag)
        .load(conn)
}

/// Every uploaded image, newest first
fn all_images(conn: &mut PgConnection) -> QueryResult<Vec<Image>> {
    images::table
        .select(Image::as_select())
        .order(images::id.desc())
        .load(conn)
}

/// For every tag, how many projects use it, not counting `exclude` (the project being edited)
fn tag_usage(
    conn: &mut PgConnection,
    exclude: Option<i32>,
) -> QueryResult<std::collections::HashMap<i32, i64>> {
    let mut query = project_tag_links::table
        .group_by(project_tag_links::tag_id)
        .select((project_tag_links::tag_id, diesel::dsl::count_star()))
        .into_boxed();

    if let Some(id) = exclude {
        query = query.filter(project_tag_links::project_id.ne(id));
    }

    Ok(query.load::<(i32, i64)>(conn)?.into_iter().collect())
}

/// Replaces all of the project's tag links with `tag_ids`
fn set_tags(conn: &mut PgConnection, project_id: i32, tag_ids: Vec<i32>) -> QueryResult<()> {
    diesel::delete(project_tag_links::table.filter(project_tag_links::project_id.eq(project_id)))
        .execute(conn)?;

    if !tag_ids.is_empty() {
        insert_into(project_tag_links::table)
            .values(
                tag_ids
                    .into_iter()
                    .map(|tag_id| TagLink { tag_id, project_id })
                    .collect::<Vec<TagLink>>(),
            )
            .execute(conn)?;
    }

    Ok(())
}

#[get("/")]
async fn index(app: web::Data<AppState>) -> Result<Index, Error> {
    let mut conn = app.conn().await?;

    let projects = p_dsl::projects
        .filter(p_dsl::published.eq(true))
        .order(p_dsl::created.desc())
        .select(Project::as_select())
        .load(&mut *conn)
        .map_err(db_err)?;

    debug!("Found {} projects", projects.len());

    let description = projects
        .iter()
        .find(|p| p.pinned)
        .map(|p| meta_description(&p.short_description));

    Ok(Index {
        projects,
        site: app.site(),
        description,
    })
}

pub const TAG_PREFIX: &'static str = "tag_";

#[derive(Debug, Deserialize)]
pub struct ProjectForm {
    title: String,
    slug: Option<String>,
    short_description: String,
    post: String,
    language: Langauge,
    metadata: Option<String>,
    css: Option<String>,
    js: Option<String>,
    published: Option<String>,
    pinned: Option<String>,
    #[serde(flatten)]
    extra: HashMap<String, String>,
}

impl ProjectForm {
    fn parse(self) -> Result<(ProjectChanges, Vec<i32>), Error> {
        let tag_ids = self
            .extra
            .keys()
            .filter_map(|k| k.strip_prefix(TAG_PREFIX))
            .map(str::parse::<i32>)
            .collect::<std::result::Result<Vec<i32>, _>>()
            .map_err(|err| ErrorBadRequest(format!("Failed to parse int: {err:?}")))?;

        let metadata = non_empty(self.metadata)
            .map(|m| from_str(&m))
            .transpose()
            .map_err(|err| ErrorBadRequest(format!("Metadata is not valid JSON: {err}")))?;

        let slug = non_empty(self.slug)
            .map(|s| slugify(&s))
            .unwrap_or_else(|| slugify(&self.title));

        if slug.is_empty() {
            return Err(ErrorBadRequest("Slug can't be empty"));
        }

        Ok((
            ProjectChanges {
                slug,
                title: self.title,
                short_description: self.short_description,
                metadata,
                post: self.post,
                language: self.language,
                published: self.published.is_some(),
                pinned: self.pinned.is_some(),
                css: non_empty(self.css),
                js: non_empty(self.js),
            },
            tag_ids,
        ))
    }
}

fn check_tags_exist(conn: &mut PgConnection, tag_ids: &[i32]) -> Result<(), Error> {
    let tags = all_tags(conn).map_err(db_err)?;

    if let Some(tag) = tag_ids.iter().find(|id| !tags.iter().any(|t| t.id == **id)) {
        error!("Tag with id: {tag:?} does not exist");
        return Err(ErrorBadRequest(format!(
            "Tag with id: {tag:?} does not exist"
        )));
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct LogInForm {
    username: String,
    password: String,
}

fn render_page(status: StatusCode, template: impl Template) -> Result<HttpResponse, Error> {
    let body = template.render().map_err(ErrorInternalServerError)?;

    Ok(HttpResponse::build(status)
        .content_type(ContentType::html())
        .body(body))
}

/// The address the reverse proxy saw. It has to append to `X-Forwarded-For`, and only the last
/// entry is trusted, since earlier ones are whatever the client claimed.
fn client_ip(req: &HttpRequest) -> String {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(|ip| ip.trim().to_string())
        .filter(|ip| !ip.is_empty())
        .or_else(|| req.peer_addr().map(|addr| addr.ip().to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

#[post("/login")]
async fn login(
    req: HttpRequest,
    app: web::Data<AppState>,
    web::Form(form): Form<LogInForm>,
) -> Result<HttpResponse, Error> {
    let ip_key = format!("ip:{}", client_ip(&req));
    let user_key = format!("user:{}", form.username.trim().to_lowercase());

    let blocked = app
        .login_limiter
        .check(&ip_key, MAX_PER_IP)
        .map_err(|wait| ("IP", wait))
        .and(
            app.login_limiter
                .check(&user_key, MAX_PER_USER)
                .map_err(|wait| ("username", wait)),
        );

    if let Err((limit, wait)) = blocked {
        warn!(
            "Login blocked by {limit} rate limit for {} (user '{}'), retry in {wait:?}",
            client_ip(&req),
            form.username.trim()
        );
        let mut res = render_page(
            StatusCode::TOO_MANY_REQUESTS,
            LoginTemplate {
                site: app.site(),
                error: Some("For mange forsøk, prøv igjen seinare.".to_string()),
            },
        )?;
        res.headers_mut().insert(
            RETRY_AFTER,
            HeaderValue::from_str(&wait.as_secs().max(1).to_string())
                .map_err(ErrorInternalServerError)?,
        );
        return Ok(res);
    }

    let admin = {
        let mut conn = app.conn().await?;

        admins::table
            .select(Admin::as_select())
            .filter(a_dsl::username.eq(form.username.trim()))
            .first(&mut *conn)
            .optional()
            .map_err(db_err)?
    };

    let dummy_hash = app.dummy_hash.clone();
    let password = form.password;
    let admin = web::block(move || match admin {
        Some(admin) => admin
            .verify(password)
            .inspect_err(|err| error!("Password verification failed: {err}"))
            .unwrap_or(false)
            .then_some(admin),
        None => {
            let _ = bcrypt::verify(password, &dummy_hash);
            None
        }
    })
    .await
    .map_err(ErrorInternalServerError)?;

    let Some(admin) = admin else {
        warn!(
            "Failed login from {} for user '{}'",
            client_ip(&req),
            form.username.trim()
        );
        app.login_limiter.fail(&ip_key);
        app.login_limiter.fail(&user_key);

        return render_page(
            StatusCode::UNAUTHORIZED,
            LoginTemplate {
                site: app.site(),
                error: Some("Feil brukarnamn eller passord.".to_string()),
            },
        );
    };

    app.login_limiter.clear(&ip_key);
    app.login_limiter.clear(&user_key);

    info!("Login by '{}' from {}", admin.username, client_ip(&req));

    let token = issue_token(&app.secret, &admin.username).map_err(|err| {
        error!("Failed to issue token: {err}");
        ErrorInternalServerError(err)
    })?;

    let cookie = Cookie::build(TOKEN, token)
        .path("/")
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(!cfg!(debug_assertions))
        .max_age(time::Duration::hours(12))
        .finish();

    Ok(HttpResponse::SeeOther()
        .cookie(cookie)
        .insert_header((LOCATION, "/projects"))
        .finish())
}

#[get("/login")]
async fn login_page(app: web::Data<AppState>) -> LoginTemplate {
    LoginTemplate {
        site: app.site(),
        error: None,
    }
}

#[post("/logout")]
async fn logout() -> HttpResponse {
    let mut cookie = Cookie::build(TOKEN, "").path("/").finish();
    cookie.make_removal();

    HttpResponse::SeeOther()
        .cookie(cookie)
        .insert_header((LOCATION, "/projects"))
        .finish()
}

#[derive(Debug, Deserialize)]
pub struct TagFilter {
    tag: Option<String>,
    page: Option<i64>,
}

const PROJECTS_PER_PAGE: i64 = 10;

/// Drafts only for the admin, and only projects with `tag` when given
fn filtered_projects(
    admin: bool,
    tag: Option<String>,
) -> projects::BoxedQuery<'static, diesel::pg::Pg> {
    let mut query = p_dsl::projects.into_boxed();

    if !admin {
        query = query.filter(p_dsl::published.eq(true));
    }

    if let Some(tag) = tag {
        query = query.filter(
            p_dsl::id.eq_any(
                project_tag_links::table
                    .inner_join(project_tags::table)
                    .filter(project_tags::tag.eq(tag))
                    .select(project_tag_links::project_id),
            ),
        );
    }

    query
}

/// Public list of published projects (optionally filtered by `?tag=`), `PROJECTS_PER_PAGE` at a time.
/// Logged in admins also see drafts, and get edit/publish/pin/delete controls.
#[get("/projects")]
async fn projects_admin(
    auth: Option<ProjectWriter>,
    app: web::Data<AppState>,
    web::Query(filter): web::Query<TagFilter>,
) -> Result<ProjectsTemplate, Error> {
    let mut conn = app.conn().await?;

    let admin = auth.and_then(|ProjectWriter(claims)| {
        a_dsl::admins
            .select(Admin::as_select())
            .filter(a_dsl::username.eq(claims.sub))
            .first(&mut *conn)
            .ok()
    });

    let total = filtered_projects(admin.is_some(), filter.tag.clone())
        .count()
        .get_result::<i64>(&mut *conn)
        .map_err(db_err)?;
    let total_pages = ((total + PROJECTS_PER_PAGE - 1) / PROJECTS_PER_PAGE).max(1);
    let page = filter.page.unwrap_or(1).clamp(1, total_pages);

    let projects = filtered_projects(admin.is_some(), filter.tag.clone())
        .order((p_dsl::pinned.desc(), p_dsl::created.desc()))
        .limit(PROJECTS_PER_PAGE)
        .offset((page - 1) * PROJECTS_PER_PAGE)
        .select(Project::as_select())
        .load(&mut *conn)
        .map_err(db_err)?;

    let tags = all_tags(&mut conn).map_err(db_err)?;

    Ok(ProjectsTemplate {
        projects,
        admin,
        tags,
        site: app.site(),
        tag: filter.tag,
        page,
        total_pages,
    })
}

#[get("/project/new")]
async fn new_project_page(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
) -> Result<ProjectFormTemplate, Error> {
    let mut conn = app.conn().await?;

    let usage = tag_usage(&mut conn, None).map_err(db_err)?;
    let tags = all_tags(&mut conn)
        .map_err(db_err)?
        .into_iter()
        .map(|tag| {
            let used = usage.get(&tag.id).copied().unwrap_or(0);
            (tag, false, used)
        })
        .collect();

    Ok(ProjectFormTemplate {
        project: Project::default(),
        tags,
        language: Langauge::default(),
        images: all_images(&mut conn).map_err(db_err)?,
        action: "/project/new".to_string(),
        site: app.site(),
    })
}

#[post("/project/new")]
async fn new_project(
    auth: ProjectWriter,
    app: web::Data<AppState>,
    web::Form(form): Form<ProjectForm>,
) -> Result<HttpResponse, Error> {
    let (changes, tag_ids) = form.parse()?;

    let mut conn = app.conn().await?;

    check_tags_exist(&mut conn, &tag_ids)?;

    let admin_id = a_dsl::admins
        .filter(a_dsl::username.eq(auth.0.sub))
        .select(a_dsl::id)
        .first::<i32>(&mut *conn)
        .map_err(db_err)?;

    let new_project = InsertProject {
        slug: changes.slug,
        title: changes.title,
        short_description: changes.short_description,
        metadata: changes.metadata,
        post: changes.post,
        language: changes.language,
        published: changes.published,
        pinned: changes.pinned,
        css: changes.css,
        js: changes.js,
        admin_id,
    };

    let project = conn
        .transaction(|conn| {
            let project = insert_into(projects::table)
                .values(new_project)
                .returning(Project::as_returning())
                .get_result(conn)?;

            set_tags(conn, project.id, tag_ids)?;

            Ok::<_, DieselError>(project)
        })
        .map_err(db_err)?;

    info!(
        "Created project '{}' (published: {})",
        project.slug, project.published
    );
    debug!("Saved project to db: {project:?}");

    Ok(if project.published {
        redirect(&format!("/project/{}", project.slug))
    } else {
        redirect("/projects")
    })
}

#[get("/project/{slug}/edit")]
async fn edit_project_page(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    slug: web::Path<String>,
) -> Result<ProjectFormTemplate, Error> {
    let mut conn = app.conn().await?;

    let project = project_by_slug(&mut conn, &slug).map_err(db_err)?;
    let project_tags = tags_of(&mut conn, project.id).map_err(db_err)?;

    let usage = tag_usage(&mut conn, Some(project.id)).map_err(db_err)?;
    let tags = all_tags(&mut conn)
        .map_err(db_err)?
        .into_iter()
        .map(|tag| {
            let checked = project_tags.iter().any(|t| t.id == tag.id);
            let used = usage.get(&tag.id).copied().unwrap_or(0);
            (tag, checked, used)
        })
        .collect();

    Ok(ProjectFormTemplate {
        action: format!("/project/{}/edit", project.slug),
        language: project.language,
        project,
        tags,
        images: all_images(&mut conn).map_err(db_err)?,
        site: app.site(),
    })
}

#[post("/project/{slug}/edit")]
async fn edit_project(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    slug: web::Path<String>,
    web::Form(form): Form<ProjectForm>,
) -> Result<HttpResponse, Error> {
    let (changes, tag_ids) = form.parse()?;

    let mut conn = app.conn().await?;

    check_tags_exist(&mut conn, &tag_ids)?;

    let existing = project_by_slug(&mut conn, &slug).map_err(db_err)?;

    let project = conn
        .transaction(|conn| {
            let project = diesel::update(p_dsl::projects.find(existing.id))
                .set((changes, p_dsl::edited.eq(now)))
                .returning(Project::as_returning())
                .get_result(conn)?;

            set_tags(conn, project.id, tag_ids)?;

            Ok::<_, DieselError>(project)
        })
        .map_err(db_err)?;

    info!("Edited project '{}'", project.slug);

    Ok(if project.published {
        redirect(&format!("/project/{}", project.slug))
    } else {
        redirect("/projects")
    })
}

/// htmx: removes the project row (empty response replaces it)
#[post("/project/{slug}/delete")]
async fn delete_project(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    slug: web::Path<String>,
) -> Result<HttpResponse, Error> {
    let mut conn = app.conn().await?;

    let deleted = diesel::delete(p_dsl::projects.filter(p_dsl::slug.eq(slug.as_str())))
        .execute(&mut *conn)
        .map_err(db_err)?;

    if deleted == 0 {
        return Err(ErrorNotFound("Not found"));
    }

    info!("Deleted project '{slug}'");

    Ok(HttpResponse::Ok().finish())
}

/// htmx: flips `published` or `pinned` and returns the updated row
#[post("/project/{slug}/toggle/{field}")]
async fn toggle_project(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    path: web::Path<(String, String)>,
) -> Result<AdminProjectRow, Error> {
    let (slug, field) = path.into_inner();
    let mut conn = app.conn().await?;

    let target = p_dsl::projects.filter(p_dsl::slug.eq(slug));

    let project = match field.as_str() {
        "published" => diesel::update(target)
            .set(p_dsl::published.eq(not(p_dsl::published)))
            .returning(Project::as_returning())
            .get_result(&mut *conn),
        "pinned" => diesel::update(target)
            .set(p_dsl::pinned.eq(not(p_dsl::pinned)))
            .returning(Project::as_returning())
            .get_result(&mut *conn),
        _ => return Err(ErrorBadRequest("Can only toggle 'published' or 'pinned'")),
    }
    .map_err(db_err)?;

    info!(
        "Toggled {field} on project '{}' (published: {}, pinned: {})",
        project.slug, project.published, project.pinned
    );

    Ok(AdminProjectRow { project })
}

#[derive(Debug, Deserialize)]
pub struct PreviewForm {
    post: String,
}

/// htmx: renders markdown for the live preview next to the editor
#[post("/preview")]
async fn preview(_auth: ProjectWriter, web::Form(form): Form<PreviewForm>) -> HttpResponse {
    html(render_markdown(&form.post))
}

/// Renders the whole project page from the unsaved form, to be shown in the editor's preview
/// iframe. Nothing is written to the database.
#[post("/preview/full")]
async fn preview_full(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    web::Form(form): Form<ProjectForm>,
) -> Result<HttpResponse, Error> {
    let (changes, _tag_ids) = form.parse()?;

    let project = Project {
        slug: changes.slug,
        title: changes.title,
        short_description: changes.short_description,
        metadata: changes.metadata,
        post: changes.post,
        language: changes.language,
        published: changes.published,
        pinned: changes.pinned,
        css: changes.css,
        js: changes.js,
        ..Default::default()
    };

    let mut res = render_page(
        StatusCode::OK,
        ProjectTemplate {
            description: meta_description(&project.short_description),
            language: project.language,
            project,
            tags: Vec::new(),
            is_admin: false,
            site: app.site(),
        },
    )?;

    // Only the editor itself may frame this, and the project's own script is inline
    let headers = res.headers_mut();
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("SAMEORIGIN"));
    headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP_PREVIEW));

    Ok(res)
}

#[derive(Debug, Deserialize)]
pub struct NewTagForm {
    new_tag: String,
}

/// htmx: creates a tag (or finds the existing one) and returns its checkbox, checked
#[post("/api/tags")]
async fn new_tag(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    web::Form(form): Form<NewTagForm>,
) -> Result<TagCheckbox, Error> {
    let name = form.new_tag.trim().to_string();

    if name.is_empty() {
        return Err(ErrorBadRequest("Tag can't be empty"));
    }

    let mut conn = app.conn().await?;

    let existing = project_tags::table
        .filter(project_tags::tag.eq(&name))
        .select(Tag::as_select())
        .first(&mut *conn)
        .optional()
        .map_err(db_err)?;

    let tag = match existing {
        Some(tag) => tag,
        None => {
            let tag = insert_into(project_tags::table)
                .values(NewTag { tag: name })
                .returning(Tag::as_returning())
                .get_result(&mut *conn)
                .map_err(db_err)?;
            info!("Created tag '{}'", tag.tag);
            tag
        }
    };

    Ok(TagCheckbox {
        tag,
        checked: true,
        used_by_others: 0,
    })
}

/// htmx: deletes the tag, which also unlinks it from every project (empty response removes it)
#[post("/api/tags/{id}/delete")]
async fn delete_tag(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    id: web::Path<i32>,
) -> Result<HttpResponse, Error> {
    let mut conn = app.conn().await?;

    diesel::delete(project_tags::table.find(*id))
        .execute(&mut *conn)
        .map_err(db_err)?;

    info!("Deleted tag {}", *id);

    Ok(HttpResponse::Ok().finish())
}

#[get("/project/{slug}")]
async fn get_project_web(
    auth: Option<ProjectWriter>,
    app: web::Data<AppState>,
    slug: web::Path<String>,
) -> Result<HttpResponse, Error> {
    let mut conn = app.conn().await?;

    let project = project_by_slug(&mut conn, &slug).map_err(db_err)?;
    let is_admin = auth.is_some();

    // Drafts are only visible to the admin
    if !project.published && !is_admin {
        return Err(ErrorNotFound("Not found"));
    }

    let tags = tags_of(&mut conn, project.id).map_err(db_err)?;
    let has_js = project.js.is_some();

    let mut res = render_page(
        StatusCode::OK,
        ProjectTemplate {
            description: meta_description(&project.short_description),
            language: project.language,
            project,
            tags,
            is_admin,
            site: app.site(),
        },
    )?;

    // The project's own script is inline, so this page has to allow that
    if has_js {
        res.headers_mut().insert(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP_INLINE_SCRIPTS),
        );
    }

    Ok(res)
}

#[get("/robots.txt")]
async fn robots(app: web::Data<AppState>) -> HttpResponse {
    HttpResponse::Ok().content_type(ContentType::plaintext()).body(format!(
        "User-agent: *\nDisallow: /login\nDisallow: /admin\nDisallow: /preview\n\nSitemap: {}/sitemap.xml\n",
        app.site_url
    ))
}

#[get("/sitemap.xml")]
async fn sitemap(app: web::Data<AppState>) -> Result<HttpResponse, Error> {
    let slugs = {
        let mut conn = app.conn().await?;

        p_dsl::projects
            .filter(p_dsl::published.eq(true))
            .order(p_dsl::created.desc())
            .select(p_dsl::slug)
            .load::<String>(&mut *conn)
            .map_err(db_err)?
    };

    // Slugs are only alphanumerics and dashes, so nothing here needs XML escaping
    let urls = ["".to_string(), "/projects".to_string()]
        .into_iter()
        .chain(slugs.into_iter().map(|slug| format!("/project/{slug}")))
        .map(|path| format!("  <url><loc>{}{path}</loc></url>\n", app.site_url))
        .collect::<String>();

    Ok(HttpResponse::Ok()
        .content_type("application/xml")
        .body(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n{urls}</urlset>\n"
        )))
}

#[derive(MultipartForm)]
pub struct UploadForm {
    slug: Option<Text<String>>,
    alt: Text<String>,
    file: TempFile,
}

const IMAGE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "avif"];

/// htmx: stores the file in the upload dir and returns the new gallery item
#[post("/images")]
async fn upload_image(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    MultipartForm(form): MultipartForm<UploadForm>,
) -> Result<ImageItem, Error> {
    let file_name = form.file.file_name.clone().unwrap_or_default();
    let (stem, ext) = file_name
        .rsplit_once('.')
        .map(|(stem, ext)| (stem.to_string(), ext.to_lowercase()))
        .ok_or_else(|| ErrorBadRequest("File needs an extension"))?;

    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(ErrorBadRequest(format!(
            "Unsupported file type, expected one of {IMAGE_EXTENSIONS:?}"
        )));
    }

    let slug = slugify(
        non_empty(form.slug.map(|s| s.0))
            .as_deref()
            .unwrap_or(&stem),
    );

    if slug.is_empty() {
        return Err(ErrorBadRequest("Slug can't be empty"));
    }

    let new_image = NewImage {
        // The slug is only alphanumerics and dashes, so this can't escape the upload dir
        img: format!("{slug}.{ext}"),
        slug,
        alt: form.alt.0,
    };
    let path = app.upload_dir.join(&new_image.img);

    let mut conn = app.conn().await?;

    // Insert first, so a taken slug can't overwrite an existing file
    let image = insert_into(images::table)
        .values(new_image)
        .returning(Image::as_returning())
        .get_result(&mut *conn)
        .map_err(db_err)?;

    if let Err(err) = tokio::fs::copy(form.file.file.path(), &path).await {
        error!("Failed to store image at {path:?}: {err}");
        diesel::delete(images::table.find(image.id))
            .execute(&mut *conn)
            .map_err(db_err)?;
        return Err(ErrorInternalServerError(err));
    }

    info!("Uploaded image '{}'", image.slug);

    Ok(ImageItem { image })
}

#[get("/image/{slug}")]
async fn get_image(app: web::Data<AppState>, slug: web::Path<String>) -> Result<NamedFile, Error> {
    let img = {
        let mut conn = app.conn().await?;

        images::table
            .filter(images::slug.eq(slug.as_str()))
            .select(images::img)
            .first::<String>(&mut *conn)
            .map_err(db_err)?
    };

    Ok(NamedFile::open(app.upload_dir.join(img))?)
}

#[post("/images/{slug}/delete")]
async fn delete_image(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    slug: web::Path<String>,
) -> Result<HttpResponse, Error> {
    let mut conn = app.conn().await?;

    let img = diesel::delete(images::table.filter(images::slug.eq(slug.as_str())))
        .returning(images::img)
        .get_result::<String>(&mut *conn)
        .map_err(db_err)?;

    if let Err(err) = tokio::fs::remove_file(app.upload_dir.join(img)).await {
        error!("Failed to remove image file: {err}");
    }

    info!("Deleted image '{slug}'");

    Ok(HttpResponse::Ok().finish())
}

const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

const CSP: &str ="default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'";

/// For project pages with their own inline script
/// The editor's preview iframe: like `CSP_INLINE_SCRIPTS`, but framable by our own pages
const CSP_PREVIEW: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'self'";
const CSP_INLINE_SCRIPTS: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'";

/// Renders the 404 page, also for paths no route matches. htmx requests keep the plain response,
/// since a whole page would end up inside a row or a form.
fn not_found<B>(res: ServiceResponse<B>) -> actix_web::Result<ErrorHandlerResponse<B>> {
    let app = res.request().app_data::<web::Data<AppState>>().cloned();
    let htmx = res.request().headers().contains_key("hx-request");

    let Some(app) = app.filter(|_| !htmx) else {
        return Ok(ErrorHandlerResponse::Response(res.map_into_left_body()));
    };

    let body = NotFoundTemplate { site: app.site() }
        .render()
        .map_err(ErrorInternalServerError)?;
    let (req, _) = res.into_parts();
    let res = HttpResponse::NotFound()
        .content_type(ContentType::html())
        .body(body)
        .map_into_right_body();

    Ok(ErrorHandlerResponse::Response(ServiceResponse::new(req, res)))
}

/// Security headers on every response (unless the handler already set one), and caching of
/// static files and images
async fn harden(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let path = req.path().to_string();
    let mut res = next.call(req).await?;

    let cache = if path.starts_with("/static/htmx-") {
        // The file name has the version in it, so it never changes
        Some("public, max-age=31536000, immutable")
    } else if path.starts_with("/static/") || path.starts_with("/image/") {
        Some("public, max-age=3600")
    } else {
        None
    };

    let mut defaults = vec![
        (CONTENT_SECURITY_POLICY, CSP),
        (STRICT_TRANSPORT_SECURITY, "max-age=31536000"),
        (X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (X_FRAME_OPTIONS, "DENY"),
        (REFERRER_POLICY, "strict-origin-when-cross-origin"),
        (
            HeaderName::from_static("permissions-policy"),
            "camera=(), microphone=(), geolocation=()",
        ),
    ];

    if let Some(cache) = cache.filter(|_| res.status().is_success()) {
        defaults.push((CACHE_CONTROL, cache));
    }

    let headers = res.headers_mut();

    for (name, value) in defaults {
        if !headers.contains_key(&name) {
            headers.insert(name, HeaderValue::from_static(value));
        }
    }

    Ok(res)
}

fn required_env(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("Environment variable {name} has to be set"))
}

fn run_migrations(db_url: &str) -> Result<()> {
    let mut conn = PgConnection::establish(db_url).context("Failed to connect to the database")?;

    let applied = conn
        .run_pending_migrations(MIGRATIONS)
        .map_err(|err| anyhow!("Failed to run migrations: {err}"))?;

    for version in applied {
        info!("Applied migration {version}");
    }

    Ok(())
}

fn set_password(db_url: &str, username: &str) -> Result<()> {
    let password = rpassword::prompt_password("New password: ")?;

    if password.chars().count() < 12 {
        bail!("Use at least 12 characters");
    }

    if rpassword::prompt_password("Repeat password: ")? != password {
        bail!("The passwords don't match");
    }

    let hash = bcrypt::hash(&password, COST)?;
    let mut conn = PgConnection::establish(db_url).context("Failed to connect to the database")?;

    insert_into(admins::table)
        .values((a_dsl::username.eq(username), a_dsl::pswd.eq(&hash)))
        .on_conflict(a_dsl::username)
        .do_update()
        .set(a_dsl::pswd.eq(&hash))
        .execute(&mut conn)?;

    println!("Password set for '{username}'");

    Ok(())
}

#[actix_web::main]
async fn main() -> Result<()> {
    dotenv().ok();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let db_url = required_env("DATABASE_URL")?;

    run_migrations(&db_url)?;

    let mut args = env::args().skip(1);
    if args.next().as_deref() == Some("set-password") {
        let username = args
            .next()
            .context("Usage: cv-webpage set-password <username>")?;
        return set_password(&db_url, &username);
    }

    // Debug builds only: `seed` is run by scripts/dev-run.sh (the cargo runner) on `cargo run`
    #[cfg(debug_assertions)]
    if env::args().nth(1).as_deref() == Some("seed") {
        let upload_dir =
            PathBuf::from(env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_string()));
        let mut conn = PgConnection::establish(&db_url)?;
        if env::args().nth(2).as_deref() != Some("--if-empty") || seed::is_empty(&mut conn)? {
            seed::seed(&mut conn, &upload_dir)?;
        }
        return Ok(());
    }

    let secret = required_env("JWT_SECRET")?;
    if secret.len() < 32 {
        bail!("JWT_SECRET has to be at least 32 characters, e.g. from `openssl rand -base64 48`");
    }

    let site_url = required_env("SITE_URL")?.trim_end_matches('/').to_string();
    if !site_url.starts_with("http://") && !site_url.starts_with("https://") {
        bail!("SITE_URL has to start with http:// or https://");
    }

    let upload_dir =
        PathBuf::from(env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_string()));
    std::fs::create_dir_all(&upload_dir)?;
    info!("Storing uploads in {upload_dir:?}");

    let pool = Pool::builder()
        .max_size(8)
        .connection_timeout(Duration::from_secs(5))
        .build(ConnectionManager::<PgConnection>::new(&db_url))
        .context("Failed to connect to the database")?;

    let state = web::Data::new(AppState {
        pool,
        secret,
        upload_dir,
        site_url,
        login_limiter: LoginLimiter::default(),
        dummy_hash: bcrypt::hash("dummy", COST)?,
        site: RwLock::new(Site::default()),
    });

    state.refresh_site(&mut *state.pool.get()?)?;

    info!("Listening on 127.0.0.1:8080");

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .wrap(ErrorHandlers::new().handler(StatusCode::NOT_FOUND, not_found))
            .wrap(from_fn(harden))
            .wrap(Logger::default())
            .service(index)
            .service(login_page)
            .service(login)
            .service(logout)
            .service(projects_admin)
            .service(new_project_page)
            .service(new_project)
            .service(get_project_web)
            .service(edit_project_page)
            .service(edit_project)
            .service(delete_project)
            .service(toggle_project)
            .service(preview)
            .service(preview_full)
            .service(new_tag)
            .service(delete_tag)
            .service(upload_image)
            .service(get_image)
            .service(delete_image)
            .service(admin::admin_page)
            .service(admin::add_link)
            .service(admin::delete_link)
            .service(admin::upload_cv)
            .service(admin::delete_cv)
            .service(admin::get_cv)
            .service(robots)
            .service(sitemap)
            .service(actix_files::Files::new("/static", "static"))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await?;

    Ok(())
}
