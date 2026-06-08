-- Add telegram as a valid source_type and service_credentials provider.
ALTER TABLE sources DROP CONSTRAINT IF EXISTS sources_source_type_check;
ALTER TABLE sources ADD CONSTRAINT sources_source_type_check
CHECK (source_type IN (
  'google_drive',
  'gmail',
  'confluence',
  'jira',
  'slack',
  'github',
  'local_files',
  'file_system',
  'web',
  'notion',
  'hubspot',
  'one_drive',
  'share_point',
  'outlook',
  'outlook_calendar',
  'ms_teams',
  'fireflies',
  'imap',
  'clickup',
  'linear',
  'paperless_ngx',
  'nextcloud',
  'telegram'
));

ALTER TABLE service_credentials DROP CONSTRAINT IF EXISTS service_credentials_provider_check;
ALTER TABLE service_credentials ADD CONSTRAINT service_credentials_provider_check
CHECK (provider IN (
  'google',
  'slack',
  'atlassian',
  'github',
  'microsoft',
  'notion',
  'hubspot',
  'fireflies',
  'imap',
  'clickup',
  'linear',
  'paperless_ngx',
  'nextcloud',
  'telegram'
));
