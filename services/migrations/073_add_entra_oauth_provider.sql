ALTER TABLE user_oauth_credentials DROP CONSTRAINT IF EXISTS user_oauth_credentials_provider_check;
ALTER TABLE user_oauth_credentials ADD CONSTRAINT user_oauth_credentials_provider_check
CHECK (provider IN ('google', 'slack', 'atlassian', 'github', 'microsoft', 'okta', 'entra'));
