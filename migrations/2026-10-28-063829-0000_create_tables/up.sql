CREATE TABLE IF NOT EXISTS admins (
  id        SERIAL PRIMARY KEY,
  username  TEXT NOT NULL UNIQUE,
  pswd      TEXT
);

CREATE TABLE IF NOT EXISTS projects (
  id                SERIAL PRIMARY KEY,
  slug              TEXT NOT NULL UNIQUE,
  title             TEXT NOT NULL,
  short_description TEXT NOT NULL,
  metadata          JSON,
  js                TEXT,
  css               TEXT,
  post              TEXT NOT NULL,
  published         BOOLEAN NOT NULL DEFAULT FALSE,
  pinned            BOOLEAN NOT NULL DEFAULT FALSE,
  created           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  edited            TIMESTAMPTZ,
  admin_id          INTEGER NOT NULL REFERENCES admins(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS images (
  id  SERIAL PRIMARY KEY,
  slug  TEXT NOT NULL UNIQUE,
  alt   TEXT NOT NULL,
  img   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS image_links (
  id          SERIAL PRIMARY KEY,
  image_id    INTEGER NOT NULL REFERENCES images(id) ON DELETE CASCADE,
  project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS image_tags (
  id  SERIAL PRIMARY KEY,
  tag TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS image_tag_links (
  id        SERIAL PRIMARY KEY,
  tag_id    INTEGER NOT NULL REFERENCES image_tags(id) ON DELETE CASCADE,
  image_id  INTEGER NOT NULL REFERENCES images(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS project_tags (
  id  SERIAL PRIMARY KEY,
  tag TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS project_tag_links (
  id          SERIAL PRIMARY KEY,
  tag_id      INTEGER NOT NULL REFERENCES project_tags(id) ON DELETE CASCADE,
  project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE
);

