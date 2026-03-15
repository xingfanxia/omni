-- TODO: We should probably not use this table for Okta, given that the oauth credentials
-- received on Okta login are not going to be used for anything post-login (unlink, say, Google
-- oauth creds which will be used for read user data).
ALTER TABLE user_oauth_credentials DROP CONSTRAINT IF EXISTS user_oauth_credentials_provider_check;
ALTER TABLE user_oauth_credentials ADD CONSTRAINT user_oauth_credentials_provider_check
CHECK (provider IN ('google', 'slack', 'atlassian', 'github', 'microsoft', 'okta'));
