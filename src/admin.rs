use crate::{
    AppState, ProjectWriter, db_err,
    models::{NewSiteLink, SiteLink},
    redirect,
    schema::site_links,
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
use diesel::{QueryDsl, RunQueryDsl, SelectableHelper, dsl::insert_into};
use log::{error, info};
use serde::Deserialize;
use std::io::Read;

const MAX_CV_BYTES: usize = 10 * 1024 * 1024;
const URL_SCHEMES: [&str; 3] = ["https://", "http://", "mailto:"];

pub fn load_links(conn: &mut diesel::PgConnection) -> diesel::QueryResult<Vec<SiteLink>> {
    site_links::table
        .select(SiteLink::as_select())
        .order(site_links::id)
        .load(conn)
}

#[get("/admin")]
pub async fn admin_page(_auth: ProjectWriter, app: web::Data<AppState>) -> AdminTemplate {
    AdminTemplate { site: app.site() }
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
