# Production upgrade acceptance — 2026-09-08

Omni is upgraded on `cl-agent-host`. Production source is `16cb24c4`, based on
upstream `24e0b619`, with the fork's Telegram, Slack, storage, search, and migration
adaptations retained.

## Running release

| Applications | Immutable image suffix |
| --- | --- |
| Searcher, indexer, connector manager | `16cb24c4` |
| AI | `9b9bc74a` |
| Web, Google, Slack, Telegram, Notion, HubSpot, web connector, sandbox | `0db16eb7` |

Every application's actual Docker image ID was compared with its intended tag.
All twelve applications run the new release. Later commits change only storage
consumers and AI health, so unaffected images retain their earlier verified tag.
Postgres, Redis, Caddy, and Docling retain their original container IDs and start
times. Data volumes and networks are unchanged.

The database advanced from migration 105 to 113. All 105 historical SHA-384
checksums remain unchanged, and a second SQLx run was a successful no-op. Before
migration, the quiesced people table had no duplicate normalized email identities,
no email values requiring normalization, and no self-manager references. See the
[rebase compatibility report](UPSTREAM-REBASE-20260908.md) for the isolated
schema/fixture tests and exact migration adaptations.

## Recovery evidence

Private recovery artifacts are in `~/omni/backups/20260908-omni-upgrade/`, mode
0700. They include a fresh approximately 10 GiB custom-format PostgreSQL dump
using zstd level 1, restore manifest, SHA-256 checksum, original and corrected
environment files, compose overrides, container inventory, and deployment logs.
A complete `pg_restore` decode to `/dev/null` validated the entire archive; this
does not claim a full data restore was performed. The slower initial gzip dump
was interrupted and retained as an explicitly named incomplete artifact. Only
`omni-before.dump` passed completed-backup checks. No production database restore
or destructive cloud action was performed.

## Corrections required for acceptance

**Content lookup I/O.** Rust TEXT parameters forced casts of indexed `CHAR(64)`
hashes and `CHAR(26)` IDs. Production lookups scanned millions of blobs; an actual
document read took 30.941 seconds. Two concurrently-created compatibility indexes
support existing and rollback images:

```sql
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_content_blobs_sha256_hash_text
  ON content_blobs ((sha256_hash::text));
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_content_blobs_id_text
  ON content_blobs ((id::text));
```

Both indexes are valid and ready; observed sizes were 58 MiB and 235 MiB. All 22
matching storage predicates now also use native character-index candidates with
an exact TEXT filter. This preserves oversized-input and trailing-space behavior
for six hash paths, thirteen scalar ID paths, and three batch ID paths. The
source correction is deployed in all three production storage consumers.
`GC_DRY_RUN=true` remains enabled.

**Public login origin.** Runtime `APP_URL` was `http://localhost:3000`, and compose
used that value for `ORIGIN`. Valid public login forms were rejected by SvelteKit
as cross-site POSTs. The authoritative runtime `.env` now uses
`APP_URL=https://omni.computelabs.ai`; both effective web values match the public
HTTPS origin. CSRF checking remains enabled. No password reset was performed.

**AI health.** Upstream replaced its model dictionary with `ProviderCache` but
left `/health` reading the removed dictionary. The endpoint now resolves the
current default provider and handles an absent embedding provider. Four focused
health regressions passed; the deployed endpoint confirms the existing configured
model provider is healthy.

## Acceptance evidence

- All production image builds passed. Actual image IDs, migration version,
  historical checksums, origin values, GC setting, and both indexes were checked.
- Twenty-two real storage query paths passed prepared-TEXT/native-index tests in
  disposable PostgreSQL fixtures. DELETE statements were only EXPLAINed; no
  deletion probe ran against production.
- Public authenticated browser login passed, and the same session survived the
  upgrade. Updated Skills navigation and authenticated search rendered; the
  coordinator observed 9,963 results with an AI answer.
- After the final storage rollout, public API health was healthy in 0.024s,
  full-text search returned three real results in 0.392s, document retrieval
  returned actual content in 0.017s, and source listing completed in 0.193s.
- Three distinct document reads measured 0.029s, 0.023s, and 0.028s after the
  compatibility fix. Unauthenticated search returned HTTP 401.
- New-release Gmail synchronization completed at `16:04:19Z` with 22 scanned and
  26 processed events. New-release Slack synchronization completed at
  `16:12:42Z` with 916 scanned and 228 processed events.
- Google Drive continues its post-upgrade synchronization with advancing counts
  and fresh heartbeats; it is not represented as completed.
- Fleet returned no Omni or cl-onyx problem during post-upgrade acceptance.

The rebase report records unchanged upstream type-check and obsolete-test
failures. Separate historical conditions remain outside this repair: 74 embedding
entries from April–May lack processing timestamps and were not manually replayed;
some legacy source/citation URLs are absent. Authenticated search and API document
content were verified, without claiming every old external source link or browser
document-preview path was validated.
