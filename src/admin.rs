use crate::{
    AppState, ProjectWriter, db_err,
    models::{NewSiteLink, SiteLink},
    redirect,
    schema::{images, projects, site_links},
    templates::project::AdminTemplate,
};
use actix_files::NamedFile;
use actix_multipart::form::{MultipartForm, tempfile::TempFile};
use actix_web::{
    Error, HttpResponse,
    error::{ErrorBadRequest, ErrorInternalServerError},
    get,
    http::header::{ContentDisposition, DispositionParam, DispositionType},
    post,
    web::{self, Form},
};
use diesel::{ExpressionMethods, QueryDsl, RunQueryDsl, SelectableHelper, dsl::insert_into};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::io::Read;

const MAX_CV_BYTES: usize = 10 * 1024 * 1024;
const URL_SCHEMES: [&str; 3] = ["https://", "http://", "mailto:"];

pub fn load_links(conn: &mut diesel::PgConnection) -> diesel::QueryResult<Vec<SiteLink>> {
    site_links::table
        .select(SiteLink::as_select())
        .order(site_links::id)
        .load(conn)
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub commit: &'static str,
    pub commit_subject: &'static str,
    pub version: &'static str,
    pub profile: &'static str,
    pub uptime_seconds: u64,
    pub uptime: String,
    pub database: String,
    pub projects: i64,
    pub drafts: i64,
    pub images: i64,
}

fn human_duration(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    match (d, h, m) {
        (0, 0, 0) => format!("{s}s"),
        (0, 0, _) => format!("{m}m {s}s"),
        (0, _, _) => format!("{h}h {m}m"),
        _ => format!("{d}d {h}h {m}m"),
    }
}

async fn collect_status(app: &AppState) -> Status {
    let uptime_seconds = app.started.elapsed().as_secs();

    let counts = async {
        let mut conn = app.conn().await.map_err(|e| e.to_string())?;
        let total: i64 = projects::table
            .count()
            .get_result(&mut *conn)
            .map_err(|e| e.to_string())?;
        let published: i64 = projects::table
            .filter(projects::published.eq(true))
            .count()
            .get_result(&mut *conn)
            .map_err(|e| e.to_string())?;
        let images: i64 = images::table
            .count()
            .get_result(&mut *conn)
            .map_err(|e| e.to_string())?;
        Ok::<_, String>((total, published, images))
    }
    .await;

    let (database, projects, drafts, images) = match counts {
        Ok((total, published, images)) => ("ok".to_string(), published, total - published, images),
        Err(err) => (format!("error: {err}"), 0, 0, 0),
    };

    Status {
        commit: env!("GIT_HASH"),
        commit_subject: env!("GIT_SUBJECT"),
        version: env!("CARGO_PKG_VERSION"),
        profile: if cfg!(debug_assertions) { "debug" } else { "release" },
        uptime_seconds,
        uptime: human_duration(uptime_seconds),
        database,
        projects,
        drafts,
        images,
    }
}

/// Machine readable status, for admins only
#[get("/status")]
pub async fn status(_auth: ProjectWriter, app: web::Data<AppState>) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(("Cache-Control", "no-store"))
        .json(collect_status(&app).await)
}

#[get("/admin")]
pub async fn admin_page(_auth: ProjectWriter, app: web::Data<AppState>) -> AdminTemplate {
    AdminTemplate {
        site: app.site(),
        status: collect_status(&app).await,
    }
}

#[derive(Debug, Deserialize)]
pub struct LinkForm {
    label: String,
    url: String,
}

#[post("/admin/links")]
pub async fn add_link(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    web::Form(form): Form<LinkForm>,
) -> Result<HttpResponse, Error> {
    let label = form.label.trim().to_string();
    let url = form.url.trim().to_string();

    if label.is_empty() {
        return Err(ErrorBadRequest("Label can't be empty"));
    }

    if !URL_SCHEMES.iter().any(|scheme| url.starts_with(scheme)) {
        return Err(ErrorBadRequest(format!(
            "URL has to start with one of {URL_SCHEMES:?}"
        )));
    }

    let mut conn = app.conn().await?;

    insert_into(site_links::table)
        .values(NewSiteLink {
            label: label.clone(),
            url,
        })
        .execute(&mut *conn)
        .map_err(db_err)?;

    info!("Added site link '{label}'");

    app.refresh_site(&mut conn).map_err(db_err)?;

    Ok(redirect("/admin"))
}

#[post("/admin/links/{id}/delete")]
pub async fn delete_link(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    id: web::Path<i32>,
) -> Result<HttpResponse, Error> {
    let mut conn = app.conn().await?;

    diesel::delete(site_links::table.find(*id))
        .execute(&mut *conn)
        .map_err(db_err)?;

    info!("Deleted site link {}", *id);

    app.refresh_site(&mut conn).map_err(db_err)?;

    Ok(redirect("/admin"))
}

#[derive(MultipartForm)]
pub struct CvForm {
    file: TempFile,
}

#[post("/admin/cv")]
pub async fn upload_cv(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
    MultipartForm(form): MultipartForm<CvForm>,
) -> Result<HttpResponse, Error> {
    if form.file.size == 0 || form.file.size > MAX_CV_BYTES {
        return Err(ErrorBadRequest(
            "The CV has to be between 1 byte and 10 MiB",
        ));
    }

    let mut magic = [0u8; 5];
    std::fs::File::open(form.file.file.path())
        .and_then(|mut f| f.read_exact(&mut magic))
        .map_err(ErrorInternalServerError)?;

    if &magic != b"%PDF-" {
        return Err(ErrorBadRequest("That's not a PDF"));
    }

    let path = app.cv_path();
    let tmp = path.with_extension("pdf.tmp");

    let stored = tokio::fs::copy(form.file.file.path(), &tmp).await;
    let stored = match stored {
        Ok(_) => tokio::fs::rename(&tmp, &path).await,
        Err(err) => Err(err),
    };

    if let Err(err) = stored {
        error!("Failed to store CV at {path:?}: {err}");
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(ErrorInternalServerError(err));
    }

    info!("Uploaded CV ({} bytes)", form.file.size);

    let mut conn = app.conn().await?;
    app.refresh_site(&mut conn).map_err(db_err)?;

    Ok(redirect("/admin"))
}

#[post("/admin/cv/delete")]
pub async fn delete_cv(
    _auth: ProjectWriter,
    app: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    match tokio::fs::remove_file(app.cv_path()).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            error!("Failed to remove CV: {err}");
            return Err(ErrorInternalServerError(err));
        }
    }

    info!("Deleted CV");

    let mut conn = app.conn().await?;
    app.refresh_site(&mut conn).map_err(db_err)?;

    Ok(redirect("/admin"))
}

#[get("/cv.pdf")]
pub async fn get_cv(app: web::Data<AppState>) -> Result<NamedFile, Error> {
    Ok(
        NamedFile::open(app.cv_path())?.set_content_disposition(ContentDisposition {
            disposition: DispositionType::Inline,
            parameters: vec![DispositionParam::Filename(
                "nils-michael-fitjar-cv.pdf".to_string(),
            )],
        }),
    )
}
