ALTER TABLE models ADD COLUMN is_secondary BOOLEAN NOT NULL DEFAULT FALSE;
CREATE UNIQUE INDEX idx_models_single_secondary ON models (is_secondary) WHERE is_secondary = TRUE;

-- Notify AI service when default/secondary model flags change
CREATE OR REPLACE FUNCTION notify_model_provider_change() RETURNS trigger AS $$
BEGIN
    IF OLD.is_default IS DISTINCT FROM NEW.is_default
       OR OLD.is_secondary IS DISTINCT FROM NEW.is_secondary THEN
        PERFORM pg_notify('model_provider_changed', json_build_object(
            'id', NEW.id,
            'is_default', NEW.is_default,
            'is_secondary', NEW.is_secondary
        )::text);
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER model_provider_notify
    AFTER UPDATE ON models
    FOR EACH ROW
    EXECUTE FUNCTION notify_model_provider_change();

-- Notify AI service when embedding provider config changes
CREATE OR REPLACE FUNCTION notify_embedding_provider_change() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('embedding_provider_changed', json_build_object(
        'id', NEW.id,
        'is_current', NEW.is_current,
        'is_deleted', NEW.is_deleted
    )::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER embedding_provider_notify
    AFTER INSERT OR UPDATE ON embedding_providers
    FOR EACH ROW
    EXECUTE FUNCTION notify_embedding_provider_change();
