use std::io::Write;
use anyhow::{Context, Result, anyhow};
use diesel::{
    AsChangeset, Selectable,
    deserialize::{FromSql, FromSqlRow, Queryable},
    expression::AsExpression,
    pg::Pg,
    prelude::Insertable,
    serialize::{IsNull, ToSql},
};
use pulldown_cmark::{Options, Parser, html::push_html};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::schema::sql_types::Lang;

pub mod project;

#[derive(Queryable, Selectable)]
#[diesel(table_name = crate::schema::admins)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Admin {
    pub id: i32,
    pub username: String,
    pswd: Option<String>,
}

pub const COST: u32 = 12;

impl Admin {
    pub fn verify(&self, input: String) -> Result<bool> {
        self.pswd
            .as_ref()
            .ok_or_else(|| anyhow!("Missing password"))
            .and_then(|hash| bcrypt::verify(input, &hash).context("Failed to verify"))
    }
}


#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::project_tag_links)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct TagLink {
    pub tag_id: i32,
    pub project_id: i32,
}

#[derive(Debug, Queryable, Selectable)]
#[diesel(table_name = crate::schema::project_tags)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Tag {
    pub id: i32,
    pub tag: String,
}

#[derive(Insertable)]
#[diesel(table_name = crate::schema::project_tags)]
pub struct NewTag {
    pub tag: String,
}

#[derive(Debug, Queryable, Selectable)]
#[diesel(table_name = crate::schema::images)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Image {
    pub id: i32,
    pub slug: String,
    pub alt: String,
    /// File name inside the upload directory
    pub img: String,
}

#[derive(Insertable)]
#[diesel(table_name = crate::schema::images)]
pub struct NewImage {
    pub slug: String,
    pub alt: String,
    pub img: String,
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = crate::schema::site_links)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct SiteLink {
    pub id: i32,
    pub label: String,
    pub url: String,
}

#[derive(Insertable)]
#[diesel(table_name = crate::schema::site_links)]
pub struct NewSiteLink {
    pub label: String,
    pub url: String,
}

/// What every page needs to render the footer, edited on the admin page
#[derive(Debug, Clone, Default)]
pub struct Site {
    pub links: Vec<SiteLink>,
    pub has_cv: bool,
}

/// Single line, at most 160 characters, for `<meta name="description">`
pub fn meta_description(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");

    if text.chars().count() <= 160 {
        return text;
    }

    let cut: String = text.chars().take(159).collect();
    format!("{}…", cut.trim_end())
}
