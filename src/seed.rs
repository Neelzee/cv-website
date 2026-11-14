//! Dummy data for local development (debug builds only). `cargo run` seeds an empty
//! database through the runner in scripts/dev-run.sh; `cargo run -- seed` forces it
//! and is safe to repeat: existing rows are kept, missing ones are added.
//!
//! Pictures come from `SEED_IMAGES` (a `:`-separated list of files), which defaults
//! to the first `MAX_IMAGES` png/jpg files in `~/Pictures`.

use crate::{
    models::{
        COST,
        project::{InsertProject, Langauge, slugify},
    },
    schema::{
        admins, image_links, image_tag_links, image_tags, images, project_tag_links, project_tags,
        projects,
    },
};
use anyhow::Result;
use diesel::{
    Connection, ExpressionMethods, OptionalExtension, PgConnection, QueryDsl, RunQueryDsl,
    dsl::insert_into,
};
use log::{info, warn};
use std::{
    env,
    path::{Path, PathBuf},
};

const MAX_IMAGES: usize = 24;
const DEV_USER: &str = "dev";
const DEV_PASSWORD: &str = "development-password";

const TAGS: [&str; 18] = [
    "rust", "web", "htmx", "nix", "embedded", "design", "linux", "python", "typescript",
    "databases", "devops", "games", "hardware", "cli", "security", "ai", "mobile", "writing",
];

const IMAGE_TAGS: [&str; 5] = ["screenshot", "photo", "diagram", "hardware", "ui"];

const TITLES: [&str; 20] = [
    "Personal Website", "Nix Dev Shell", "Keyboard Firmware", "Nettside for Lag", "Kokebok",
    "Unfinished Draft", "Terminal Dashboard", "Home Server", "Chess Engine", "Budsjettverktøy",
    "Photo Gallery", "Static Site Generator", "Bachelor Thesis", "Smart Thermostat", "Port Scanner",
    "Recipe Recommender", "Mobile Habit Tracker", "Blogg om Rust", "Retro Platformer", "Dotfiles",
];

pub fn is_empty(conn: &mut PgConnection) -> Result<bool> {
    Ok(projects::table.count().get_result::<i64>(conn)? == 0)
}

fn image_paths() -> Vec<PathBuf> {
    if let Ok(list) = env::var("SEED_IMAGES") {
        return list.split(':').map(PathBuf::from).collect();
    }
    let dir = PathBuf::from(env::var("HOME").unwrap_or_default()).join("Pictures");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("png" | "jpg" | "jpeg")
            )
        })
        .collect();
    files.sort();
    files.truncate(MAX_IMAGES);
    files
}

pub fn seed(conn: &mut PgConnection, upload_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(upload_dir)?;

    conn.transaction::<_, anyhow::Error, _>(|conn| {
        let admin_id: i32 = match admins::table
            .filter(admins::username.eq(DEV_USER))
            .select(admins::id)
            .first(conn)
            .optional()?
        {
            Some(id) => id,
            None => insert_into(admins::table)
                .values((
                    admins::username.eq(DEV_USER),
                    admins::pswd.eq(bcrypt::hash(DEV_PASSWORD, COST)?),
                ))
                .returning(admins::id)
                .get_result(conn)?,
        };

        let tag_ids: Vec<i32> = TAGS
            .iter()
            .map(|t| {
                insert_into(project_tags::table)
                    .values(project_tags::tag.eq(*t))
                    .on_conflict(project_tags::tag)
                    .do_update()
                    .set(project_tags::tag.eq(*t))
                    .returning(project_tags::id)
                    .get_result(conn)
            })
            .collect::<Result<_, _>>()?;

        let image_tag_ids: Vec<i32> = IMAGE_TAGS
            .iter()
            .map(|t| {
                insert_into(image_tags::table)
                    .values(image_tags::tag.eq(*t))
                    .on_conflict(image_tags::tag)
                    .do_update()
                    .set(image_tags::tag.eq(*t))
                    .returning(image_tags::id)
                    .get_result(conn)
            })
            .collect::<Result<_, _>>()?;

        let mut image_ids: Vec<(String, i32)> = Vec::new();
        for (i, path) in image_paths().iter().enumerate() {
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };
            if !path.exists() {
                warn!("Seed image {path:?} not found, skipping");
                continue;
            }
            let slug = format!("seed-{}", i + 1);
            let file = format!("{slug}.{ext}");
            std::fs::copy(path, upload_dir.join(&file))?;
            let id: i32 = insert_into(images::table)
                .values((
                    images::slug.eq(&slug),
                    images::alt.eq(format!("Dummy picture {}", i + 1)),
                    images::img.eq(&file),
                ))
                .on_conflict(images::slug)
                .do_update()
                .set(images::img.eq(&file))
                .returning(images::id)
                .get_result(conn)?;

            // Re-seeding replaces the tag links instead of duplicating them
            diesel::delete(image_tag_links::table.filter(image_tag_links::image_id.eq(id)))
                .execute(conn)?;
            for k in 0..(1 + i % 2) {
                insert_into(image_tag_links::table)
                    .values((
                        image_tag_links::tag_id.eq(image_tag_ids[(i + k * 2) % IMAGE_TAGS.len()]),
                        image_tag_links::image_id.eq(id),
                    ))
                    .execute(conn)?;
            }
            image_ids.push((slug, id));
        }

        let mut created = 0;
        for (n, title) in TITLES.into_iter().enumerate() {
            let slug = slugify(title);
            let norwegian = matches!(n, 3 | 4 | 9 | 17);
            let language = if norwegian { Langauge::No } else { Langauge::En };

            // 1-3 images per project, rotating through what's available
            let linked: Vec<&(String, i32)> = (0..(1 + n % 3))
                .filter_map(|k| {
                    (!image_ids.is_empty()).then(|| &image_ids[(n * 2 + k) % image_ids.len()])
                })
                .collect();
            let images_md: String = linked
                .iter()
                .map(|(slug, _)| format!("\n![Dummy picture](/image/{slug})\n"))
                .collect();

            let inserted: Option<i32> = insert_into(projects::table)
                .values(InsertProject {
                    slug,
                    title: title.to_string(),
                    short_description: format!("Dummy project #{}: {title}.", n + 1),
                    post: format!(
                        "# {title}\n\nLorem ipsum dolor sit amet, **consectetur** adipiscing elit. \
                         Sed do eiusmod tempor incididunt ut labore.\n\n\
                         - First point\n- Second point\n\n\
                         ```rust\nfn main() {{ println!(\"hi\"); }}\n```\n{images_md}"
                    ),
                    language,
                    published: n != 5 && n % 7 != 6,
                    // Exactly one seeded project is pinned
                    pinned: n == 0,
                    admin_id,
                    ..Default::default()
                })
                .on_conflict(projects::slug)
                .do_nothing()
                .returning(projects::id)
                .get_result(conn)
                .optional()?;

            // Existing project: leave it and its links alone
            let Some(project_id) = inserted else { continue };
            created += 1;

            for k in 0..(1 + n % 4) {
                insert_into(project_tag_links::table)
                    .values((
                        project_tag_links::tag_id.eq(tag_ids[(n * 3 + k * 5) % TAGS.len()]),
                        project_tag_links::project_id.eq(project_id),
                    ))
                    .on_conflict_do_nothing()
                    .execute(conn)?;
            }
            for (_, image_id) in linked {
                insert_into(image_links::table)
                    .values((
                        image_links::image_id.eq(image_id),
                        image_links::project_id.eq(project_id),
                    ))
                    .execute(conn)?;
            }
        }

        info!(
            "Seeded {} tags, {} images and {created} new projects. Admin login: {DEV_USER} / {DEV_PASSWORD}",
            TAGS.len(),
            image_ids.len()
        );
        Ok(())
    })
}
