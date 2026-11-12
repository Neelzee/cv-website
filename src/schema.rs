// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, Clone, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "lang"))]
    pub struct Lang;
}

diesel::table! {
    admins (id) {
        id -> Int4,
        username -> Text,
        pswd -> Nullable<Text>,
    }
}

diesel::table! {
    image_links (id) {
        id -> Int4,
        image_id -> Int4,
        project_id -> Int4,
    }
}

diesel::table! {
    image_tag_links (id) {
        id -> Int4,
        tag_id -> Int4,
        image_id -> Int4,
    }
}

diesel::table! {
    image_tags (id) {
        id -> Int4,
        tag -> Text,
    }
}

diesel::table! {
    images (id) {
        id -> Int4,
        slug -> Text,
        alt -> Text,
        img -> Text,
    }
}

diesel::table! {
    project_tag_links (id) {
        id -> Int4,
        tag_id -> Int4,
        project_id -> Int4,
    }
}

diesel::table! {
    project_tags (id) {
        id -> Int4,
        tag -> Text,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::Lang;

    projects (id) {
        id -> Int4,
        slug -> Text,
        title -> Text,
        short_description -> Text,
        metadata -> Nullable<Json>,
        js -> Nullable<Text>,
        css -> Nullable<Text>,
        post -> Text,
        published -> Bool,
        pinned -> Bool,
        created -> Timestamptz,
        edited -> Nullable<Timestamptz>,
        admin_id -> Int4,
        language -> Lang,
    }
}

diesel::table! {
    site_links (id) {
        id -> Int4,
        label -> Text,
        url -> Text,
    }
}

diesel::joinable!(image_links -> images (image_id));
diesel::joinable!(image_links -> projects (project_id));
diesel::joinable!(image_tag_links -> image_tags (tag_id));
diesel::joinable!(image_tag_links -> images (image_id));
diesel::joinable!(project_tag_links -> project_tags (tag_id));
diesel::joinable!(project_tag_links -> projects (project_id));
diesel::joinable!(projects -> admins (admin_id));

diesel::allow_tables_to_appear_in_same_query!(
    admins,
    image_links,
    image_tag_links,
    image_tags,
    images,
    project_tag_links,
    project_tags,
    projects,
    site_links,
);
