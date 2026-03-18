CREATE TABLE email_providers (
    id CHAR(26) PRIMARY KEY,
    name TEXT NOT NULL,
    provider_type TEXT NOT NULL,
    config JSONB NOT NULL DEFAULT '{}',
    is_current BOOLEAN NOT NULL DEFAULT FALSE,
    is_deleted BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX idx_email_providers_single_current
    ON email_providers (is_current) WHERE is_current = TRUE AND is_deleted = FALSE;

CREATE TRIGGER set_updated_at BEFORE UPDATE ON email_providers
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
