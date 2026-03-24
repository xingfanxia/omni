ALTER TABLE models ADD COLUMN is_secondary BOOLEAN NOT NULL DEFAULT FALSE;
CREATE UNIQUE INDEX idx_models_single_secondary ON models (is_secondary) WHERE is_secondary = TRUE;
