-- Durable, selection-independent lifecycle for curated model downloads.
-- A row survives Settings remounts and app restarts; the final installed
-- artifact continues to live on `models.local_path` / `installed_at`.
CREATE TABLE model_downloads (
  model_id          TEXT PRIMARY KEY REFERENCES models(id) ON DELETE CASCADE,
  state             TEXT NOT NULL CHECK (
    state IN ('queued', 'downloading', 'verifying', 'paused', 'failed', 'stopped', 'completed')
  ),
  bytes_downloaded  INTEGER NOT NULL DEFAULT 0 CHECK (bytes_downloaded >= 0),
  bytes_total       INTEGER NOT NULL DEFAULT 0 CHECK (bytes_total >= 0),
  revision          INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
  error             TEXT,
  updated_at        INTEGER NOT NULL
);
