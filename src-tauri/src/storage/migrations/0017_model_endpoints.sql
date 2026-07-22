-- Custom model endpoints: a third model source alongside curated downloads and
-- local GGUF files -- any OpenAI-compatible URL (a local server, a hosted API,
-- or a LAN cluster). Admitting 'endpoint' into source_kind's CHECK requires
-- rebuilding the table (SQLite cannot alter a column CHECK in place), and the
-- rebuild also adds the per-endpoint columns the turn path reads. The API key
-- is NEVER stored here -- it lives in the OS secret store (see
-- commands::models::EndpointKeyStore), the same discipline OAuth tokens follow.
--
-- model_downloads has an ON DELETE CASCADE foreign key to models(id), and the
-- implicit DELETE a DROP TABLE performs cascades its rows away, so its contents
-- are parked in a TEMP table and restored after the rebuild. (PRAGMA
-- foreign_keys is a no-op inside the migration transaction, so it cannot be
-- toggled here.)
CREATE TEMP TABLE model_downloads_backup AS SELECT * FROM model_downloads;

CREATE TABLE models_new (
  id               TEXT PRIMARY KEY,
  hardware_tier    TEXT NOT NULL,
  source_url       TEXT NOT NULL,
  quantization     TEXT NOT NULL,
  sha256           TEXT NOT NULL,
  local_path       TEXT,
  capability_tags  TEXT NOT NULL DEFAULT '[]',
  installed_at     INTEGER,
  is_active        INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0, 1)),
  source_kind      TEXT NOT NULL DEFAULT 'curated'
                     CHECK (source_kind IN ('curated', 'local', 'endpoint')),
  display_name     TEXT,
  endpoint_url     TEXT,
  endpoint_model   TEXT,
  context_window   INTEGER,
  use_cache_prompt INTEGER NOT NULL DEFAULT 0
);

INSERT INTO models_new (id, hardware_tier, source_url, quantization, sha256,
                        local_path, capability_tags, installed_at, is_active,
                        source_kind, display_name)
SELECT id, hardware_tier, source_url, quantization, sha256, local_path,
       capability_tags, installed_at, is_active, source_kind, display_name
FROM models;

DROP TABLE models;
ALTER TABLE models_new RENAME TO models;

-- Enforces "exactly one active model" at the database level (data-model.md).
CREATE UNIQUE INDEX idx_models_single_active ON models(is_active) WHERE is_active = 1;

INSERT INTO model_downloads SELECT * FROM model_downloads_backup;
DROP TABLE model_downloads_backup;
