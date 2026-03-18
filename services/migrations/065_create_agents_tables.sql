-- Background/Scheduled Agents tables

CREATE TABLE IF NOT EXISTS agents (
    id CHAR(26) PRIMARY KEY,
    user_id CHAR(26) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    instructions TEXT NOT NULL,
    agent_type TEXT NOT NULL DEFAULT 'user',
    schedule_type TEXT NOT NULL,
    schedule_value TEXT NOT NULL,
    model_id CHAR(26) REFERENCES models(id) ON DELETE SET NULL,
    allowed_sources JSONB NOT NULL DEFAULT '[]',
    allowed_actions JSONB NOT NULL DEFAULT '[]',
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    is_deleted BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT agents_type_check CHECK (agent_type IN ('user', 'org')),
    CONSTRAINT agents_schedule_type_check CHECK (schedule_type IN ('cron', 'interval'))
);

CREATE INDEX IF NOT EXISTS idx_agents_user_id ON agents(user_id);
CREATE INDEX IF NOT EXISTS idx_agents_enabled ON agents(is_enabled) WHERE is_enabled = TRUE AND is_deleted = FALSE;

CREATE TRIGGER update_agents_updated_at BEFORE UPDATE ON agents
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE IF NOT EXISTS agent_runs (
    id CHAR(26) PRIMARY KEY,
    agent_id CHAR(26) NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending',
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    execution_log JSONB NOT NULL DEFAULT '[]',
    summary TEXT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT agent_runs_status_check CHECK (status IN ('pending', 'running', 'completed', 'failed'))
);

CREATE INDEX IF NOT EXISTS idx_agent_runs_agent_id ON agent_runs(agent_id);
CREATE INDEX IF NOT EXISTS idx_agent_runs_status ON agent_runs(status) WHERE status IN ('pending', 'running');
