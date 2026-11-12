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

#[derive(Debug, Queryable, Selectable, Default, Serialize, Deserialize)]
#[diesel(table_name = crate::schema::projects)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Project {
    pub id: i32,
    pub slug: String,
    pub title: String,
    pub short_description: String,
    pub metadata: Option<Value>,
    pub post: String,
    pub language: Langauge,
    pub published: bool,
    pub pinned: bool,
    pub css: Option<String>,
    pub js: Option<String>,
    pub admin_id: i32,
}

impl Project {
    pub fn render(&self) -> String {
        render_markdown(&self.post)
    }

    // Helpers for pre-filling the edit form
    pub fn metadata_text(&self) -> String {
        self.metadata
            .as_ref()
            .and_then(|m| serde_json::to_string_pretty(m).ok())
            .unwrap_or_default()
    }

    pub fn css_text(&self) -> &str {
        self.css.as_deref().unwrap_or_default()
    }

    pub fn js_text(&self) -> &str {
        self.js.as_deref().unwrap_or_default()
    }
}

#[derive(
    Default, Debug, Clone, Copy, PartialEq, Eq, AsExpression, FromSqlRow, Serialize, Deserialize,
)]
#[diesel(sql_type = Lang)]
#[serde(rename_all = "lowercase")]
pub enum Langauge {
    #[default]
    No,
    En,
}

impl ToSql<Lang, Pg> for Langauge {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, Pg>,
    ) -> diesel::serialize::Result {
        out.write_all(match self {
            Langauge::No => b"no",
            Langauge::En => b"en",
        })?;
        Ok(IsNull::No)
    }
}

impl FromSql<Lang, Pg> for Langauge {
    fn from_sql(
        bytes: <Pg as diesel::backend::Backend>::RawValue<'_>,
    ) -> diesel::deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"no" => Ok(Langauge::No),
            b"en" => Ok(Langauge::En),
            other => Err(format!("Unknown language: {}", String::from_utf8_lossy(other)).into()),
        }
    }
}

impl Langauge {
    pub fn value(&self) -> &'static str {
        match self {
            Langauge::No => "no",
            Langauge::En => "en",
        }
    }

    pub fn to_long(&self) -> String {
        match self {
            Langauge::No => "Norsk".to_string(),
            Langauge::En => "English".to_string(),
        }
    }

    pub fn languages() -> Vec<Self> {
        vec![
            Self::No,
            Self::En
        ]
    }
}

#[derive(Insertable, Default)]
#[diesel(table_name = crate::schema::projects)]
pub struct InsertProject {
    pub slug: String,
    pub title: String,
    pub short_description: String,
    pub metadata: Option<Value>,
    pub post: String,
    pub language: Langauge,
    pub published: bool,
    pub pinned: bool,
    pub css: Option<String>,
    pub js: Option<String>,
    pub admin_id: i32,
}

/// `treat_none_as_null` so that clearing metadata/css/js in the edit form actually clears them.
#[derive(AsChangeset)]
#[diesel(table_name = crate::schema::projects, treat_none_as_null = true)]
pub struct ProjectChanges {
    pub slug: String,
    pub title: String,
    pub short_description: String,
    pub metadata: Option<Value>,
    pub post: String,
    pub language: Langauge,
    pub published: bool,
    pub pinned: bool,
    pub css: Option<String>,
    pub js: Option<String>,
}

pub const OPTIONS: Options = Options::all();

pub fn render_markdown(post: &str) -> String {
    let mut buf = String::new();
    push_html(&mut buf, Parser::new_ext(post, OPTIONS));
    buf
}

/// Lowercase, alphanumerics kept, everything else collapsed into single dashes.
pub fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}