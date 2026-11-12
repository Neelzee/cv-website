CREATE TYPE lang as ENUM (
  'no',
  'en'
);

ALTER TABLE projects ADD COLUMN language lang NOT NULL DEFAULT 'no';