-- Add telegram as a valid source_type and service_credentials provider.
--
-- NOTE (fork): rewritten after the 2026-07 upstream rebase to be the UNION of
-- upstream migration 100 (google_chat / google_ads / darwinbox) plus telegram.
-- This migration runs last, so its constraint definition is authoritative —
-- it must always be a superset of the latest upstream rewrite.
ALTER TABLE sources DROP CONSTRAINT IF EXISTS sources_source_type_check;
ALTER TABLE sources ADD CONSTRAINT sources_source_type_check
CHECK (source_type IN (
  'google_drive',
  'gmail',
  'google_chat',
  'confluence',
  'jira',
  'slack',
  'notion',
  'web',
  'github',
  'local_files',
  'file_system',
  'fireflies',
  'hubspot',
  'one_drive',
  'share_point',
  'outlook',
  'outlook_calendar',
  'imap',
  'clickup',
  'linear',
  'ms_teams',
  'paperless_ngx',
  'nextcloud',
  'google_ads',
  'darwinbox',
  'telegram'
));

ALTER TABLE service_credentials DROP CONSTRAINT IF EXISTS service_credentials_provider_check;
ALTER TABLE service_credentials ADD CONSTRAINT service_credentials_provider_check
CHECK (provider IN (
  'google',
  'slack',
  'atlassian',
  'github',
  'notion',
  'fireflies',
  'hubspot',
  'microsoft',
  'imap',
  'clickup',
  'linear',
  'paperless_ngx',
  'nextcloud',
  'google_ads',
  'darwinbox',
  'telegram'
));
