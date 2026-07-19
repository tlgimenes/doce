-- Model selector: distinguish managed registry downloads from user-selected
-- files while preserving every pre-existing row as a curated model.
ALTER TABLE models
ADD COLUMN source_kind TEXT NOT NULL DEFAULT 'curated'
CHECK (source_kind IN ('curated', 'local'));

ALTER TABLE models
ADD COLUMN display_name TEXT;
