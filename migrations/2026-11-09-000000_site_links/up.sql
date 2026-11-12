CREATE TABLE IF NOT EXISTS site_links (
  id     SERIAL PRIMARY KEY,
  label  TEXT NOT NULL,
  url    TEXT NOT NULL
);

INSERT INTO site_links (label, url) VALUES
  ('GitHub', 'https://github.com/Neelzee'),
  ('LinkedIn', 'https://www.linkedin.com/in/nils-michael-fitjar-4346a3234/'),
  ('nilsien2001@gmail.com', 'mailto:nilsien2001@gmail.com');
