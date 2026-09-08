# Upstream rebase, 2026-09-08

The fork is rebased onto `getomnico/omni` master `24e0b619` (2026-09-07),
preserving the 27 fork commits through the content-hash lookup repair. This is
a source branch. Production remains on its existing immutable images, with the
online content-hash expression index mitigating the database slowdown.

## Why update

The 91 incoming commits contain useful improvements beyond this incident:

- `5d88846a`: dependency updates addressing denial of service, path traversal,
  and memory exhaustion advisories.
- `e1e69663`, migration 106 upstream: scope integration-page sync history and
  index latest-run lookups.
- `3a259360`: parallel connector health requests and avoid duplicate requests
  when loading the integrations page.
- `cdd5a5f4`: isolate model clients and invalidate them when their configuration
  changes; avoid blocking the event loop on Bedrock calls.
- `acdbb60a`: fix unsupported ParadeDB 0.24 chat-search query shapes.

Upstream still contains the TEXT-versus-CHAR content-hash bug that caused the
September 8 I/O amplification. The fork's six explicit parameter casts and
exact-text filters remain necessary.

## Rebase adaptations

Telegram remains supported alongside Windshift and remote MCP in Rust model
conversion, the web settings UI, and Docker configuration. The fork's pending-only
queue summaries now preserve the upstream separate person-mutation dequeue path;
document readiness excludes person events and terminal history while counting
both blob and inline payload bytes. Slack heartbeat, bounded/resumable downloads,
GC safeguards, and the ParadeDB search fixes remain in the rebased history.

The production database's 105 applied migrations were compared by SHA-384
against live `_sqlx_migrations` metadata. All remain byte-for-byte unchanged.
Upstream migrations 105–112 are renumbered 106–113 as one ordered suffix:

| Fork version | Change |
| --- | --- |
| 106 | Skills |
| 107 | Source/start-time sync-run index |
| 108 | Remote MCP; preserve Telegram in both source/provider constraints |
| 109 | Windshift; preserve Telegram in both source/provider constraints |
| 110 | People source data and identity normalization |
| 111 | People contact fields |
| 112 | Chat-message error persistence |
| 113 | Generic task queue |

The only other SQL adaptation is `DROP INDEX IF EXISTS people_search_idx` in
110. The production schema lacks this index; upstream's unconditional drop
fails before it can rebuild the intended BM25 index.

## Verification

- `cargo check` passed for `shared`, `omni-indexer`,
  `omni-connector-manager`, `omni-google-connector`, and `omni-slack-connector`.
  The build host's missing OpenSSL headers were supplied from an unpacked
  package in `/tmp`; no system packages or live services were changed.
- Web production build passed.
- The upstream provider contract and selected provider/chunking tests passed:
  31 tests. Nine older OpenAI tests still pass removed `temperature`/`top_p`
  arguments; the failing files and implementation are unchanged from upstream.
- Svelte type checking reports the same 59 errors as an unmodified checkout of
  `24e0b619`; an exact path/message comparison found no added errors. Upstream
  has 140 warnings and the fork 144, including the retained Telegram UI.
- All six real storage queries passed TEXT/generic-plan regression checks,
  including exact lookup, oversized input, and trailing-space semantics. The
  same regression rejects the original uncast lookup.
- The actual rebased pending-summary SQL passed a temporary-table PostgreSQL
  check for terminal-history exclusion, person-event exclusion, and payload
  byte accounting. The retained Rust regression expectation now includes inline
  payload bytes, matching the upstream queue contract.
- The migration verifier restored production schema metadata into a disposable
  ParadeDB 0.24.2 database, seeded synthetic fixtures, and ran SQLx from 105
  through 113. Telegram/Windshift/MCP constraints, invalid-value rejection,
  people deduplication with field and manager preservation, historical
  checksums, and a second no-op migration run all passed. No production data
  was copied or mutated; temporary test containers were removed.

Reproduce the migration gate using read-only schema and checksum exports:

```bash
python3 scripts/ops/verify-upstream-migration-compat.py \
  --applied-checksums /path/to/production-migration-checksums.tsv \
  --schema-dump /path/to/production-schema-only.sql
```

## Production rollout boundary

No production migration or image rollout is included in this source rebase.
Migration 110 can merge/delete normalized-email duplicates, rewrite manager
references, and rebuild the people search index. At the September 8 read-only
check, 25,188 people had zero duplicate identities, zero emails requiring
normalization, and zero self-manager references. Refresh these counts, verify a
restorable database backup, and review the exact data impact before rollout.
Keep the content-hash compatibility index and `GC_DRY_RUN=true` throughout.
