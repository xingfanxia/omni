# Omni Ops Runbook (cl-onyx production)

_Last verified: 2026-09-08 (upstream rebase deployed; source `16cb24c4`)._

## Layout

- Checkout: `~/omni/repo`, detached at `16cb24c4`. Immutable component image pins,
  migration compatibility, backup, login-origin repair, and live acceptance are
  recorded in the [production upgrade report](PRODUCTION-UPGRADE-20260908.md).
- Compose: run from `~/omni/repo/docker/` with `--env-file ../.env` (root `.env` is untracked; holds secrets + `OMNI_VERSION` + `ENABLED_CONNECTORS` profiles)
- `docker/docker-compose.override.yml` (untracked) pins locally-built custom images.
  The root `.env`, override, and their backups are runtime state and stay
  untracked. Use immutable per-component commit tags for new builds so rollback
  does not depend on a mutable `latest` tag.
- `OMNI_VERSION=latest` since 2026-07-11 — upstream stopped cutting semver at 0.1.2; each ghcr image's `latest` = its newest master build. Pull deliberately, never blind-restart onto a fresh pull.

## Build custom images (from repo root)

```bash
docker build -f services/<svc>/Dockerfile -t omni-<svc>-custom:latest .   # searcher/indexer/connector-manager/migrations(migrator)
docker build -f connectors/google/Dockerfile -t omni-google-connector-custom:latest .
docker build -f connectors/telegram/Dockerfile -t omni-telegram-connector:latest .
docker build -t omni-web-custom:latest web/                               # web builds with web/ context
```

For the bounded indexer and Slack-sync patches, build each immutable image from
its own exact commit. Do not build both from whichever commit happens to be
checked out:

```bash
git fetch origin

git worktree add --detach /tmp/omni-indexer-build d4cdb5ac
docker build -f /tmp/omni-indexer-build/services/indexer/Dockerfile \
  -t omni-indexer-custom:d4cdb5ac /tmp/omni-indexer-build
git worktree remove /tmp/omni-indexer-build

git worktree add --detach /tmp/omni-slack-build 0a44cd68
docker build -f /tmp/omni-slack-build/connectors/slack/Dockerfile \
  -t omni-slack-connector-custom:0a44cd68 /tmp/omni-slack-build
git worktree remove /tmp/omni-slack-build
```

Pin the component-specific tags in `docker/docker-compose.override.yml`. Keep
the Slack connector at `mem_limit: 512m`; the connector rejects downloads above
`SLACK_MAX_DOWNLOAD_BYTES` (50 MiB by default) before buffering them.

The GC reconciliation queries are bounded and have transaction-local statement
and lock timeouts. The storage deletion path still has a small interval between
the database candidate transaction and object-store deletion, so a first deploy
must set `GC_DRY_RUN: "true"` on the indexer. Dry-run mode still marks and
unmarks references but does not delete objects. Keep that override in place
until the deletion path gains an atomic claim/lease or outbox and passes a
separate data-safety review; operator acknowledgement alone does not close the
known race.

After recreating the indexer, verify the effective container environment before
triggering GC. Each reconciliation/fetch statement has its own 30-second
timeout, so the complete three-phase run can legitimately take longer than 30
seconds:

```bash
docker inspect omni-indexer --format '{{range .Config.Env}}{{println .}}{{end}}' \
  | grep -x 'GC_DRY_RUN=true'
indexer_port="$(docker inspect omni-indexer --format '{{range .Config.Env}}{{println .}}{{end}}' \
  | sed -n 's/^PORT=//p')"
test -n "$indexer_port"
docker run --rm --network docker_omni-network curlimages/curl -fsS \
  -X POST "http://indexer:${indexer_port}/admin/gc/run"
docker logs omni-indexer --since 10m | grep -E "garbage collection|GC completed|DRY RUN"
```

## Slack heartbeat rollout and verification

Full/incremental and realtime Slack work heartbeat every 30 seconds. Scheduled
source health must inspect only full/incremental runs: the long-lived realtime
Socket Mode watcher is a separate slot and must not hide a stale or failed
document sync. The heartbeat ticker stops when its sync future finishes, and a
post-cancel heartbeat cannot revive a non-running database row.

After pinning `omni-slack-connector-custom:0a44cd68`, recreate only the Slack
connector; do not bounce the rest of Omni:

```bash
cd ~/omni/repo/docker
docker compose --env-file ../.env up -d --no-deps slack-connector
docker inspect omni-slack-connector \
  --format '{{.Config.Image}}|{{.State.Status}}|{{.RestartCount}}'
docker logs --since 2m omni-slack-connector | grep -E \
  'HTTP server listening|Registered with connector manager|Socket Mode.*connected'
```

The image must be the exact immutable tag, state `running`, with an unchanged
restart count after stabilization. Trigger an incremental manual acceptance run
through the connector manager's JSON endpoint; the `/sync/<source_id>` shorthand
starts a full sync and is not the default check for a large Slack workspace:

```bash
docker run --rm --network docker_omni-network curlimages/curl -fsS \
  -X POST -H 'Content-Type: application/json' \
  --data '{"source_id":"<source_id>","sync_mode":"incremental"}' \
  http://connector-manager:3004/sync

docker exec omni-postgres psql -U omni -d omni -P pager=off -c \
  "SELECT id,status,last_activity_at,documents_scanned,documents_processed,
          documents_updated,completed_at,error_message
     FROM sync_runs WHERE id='<sync_run_id>';"
```

Accept either a completed successful run or an active run whose
`last_activity_at` advances across at least two 30-second intervals. For a long
first crawl, also verify the run-local checkpoint grows after completed
channels; that is the restart/resume boundary. `fleet host cl-onyx` should show
the scheduled slot as fresh/running and `fleet problems` should have no Omni
problem. A realtime watcher being healthy is not sufficient evidence.

## Fork maintenance rules (learned the hard way)

1. **Migration checksums are sacred** — `_sqlx_migrations` checksums must match file bytes. Never renumber/edit an applied migration without a matching DB row fix.
2. **Constraint-rewrite migrations must be telegram-aware** — any upstream migration that rewrites `sources_source_type_check` / `service_credentials_provider_check` will fail on our data (telegram rows exist) unless patched to include `'telegram'`. Our newest telegram migration must stay the last rewrite and be a superset of upstream's latest list (see 105).
3. **pg_search 0.24.x query-shape limit** — `pdb.score()` + `JOIN sources` + `source_id = ANY($n)` + `LIMIT >= ~50` in one scan → `Unsupported query shape`. Searcher uses `ANY(ARRAY(SELECT id FROM sources WHERE NOT is_deleted))` instead of joins in bm25 CTEs, and MATERIALIZED fences. Re-test after any paradedb bump (repro: see commit 60719cf4).
4. **After a paradedb image bump**: check `datcollversion` vs OS glibc (`docker logs omni-postgres | grep collation`); if mismatched → `ALTER DATABASE ... REFRESH COLLATION VERSION` + `REINDEX DATABASE CONCURRENTLY omni`.

## Health checks

### Content hash lookup I/O amplification (2026-09-08)

`content_blobs.sha256_hash` is `CHAR(64)`, but SQLx binds Rust strings as
PostgreSQL `TEXT`. A plain `sha256_hash = $1` consequently casts the column to
text and bypasses the existing character hash index. In production this caused
30-second full-table scans over roughly 4.9 million blobs, disk contention, and
intermittent Fleet probe timeouts. The Fleet runner kills its command after eight
seconds, explaining `sources query failed: exit 137`; this incident had no
Postgres cgroup OOM kills.

All six storage lookups now cast the parameter to `bpchar` so the existing index
is usable. They retain the original text equality as a filter to preserve exact
input semantics. Unlike `char(64)`, `bpchar` does not truncate oversized inputs.
The focused regression uses the actual source queries, TEXT prepared parameters,
and session-local temporary fixtures:

```bash
python3 scripts/ops/verify-content-hash-index.py \
  docker exec -i omni-postgres psql -U omni -d omni
```

An online compatibility index allows existing immutable images to recover before
they are rebuilt. It changes no stored rows and does not require a restart:

```sql
SET lock_timeout = '5s';
SET statement_timeout = '15min';
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_content_blobs_sha256_hash_text
  ON content_blobs ((sha256_hash::text));
```

After creation, require `pg_index.indisvalid` and `indisready`, and run
`EXPLAIN (ANALYZE, BUFFERS)` with a prepared `TEXT` parameter for the original
uncast query. Verify the new index is used and monitor Fleet across subsequent
polls and connector sync activity. If a concurrent build is interrupted, inspect
the index flags before retrying: `IF NOT EXISTS` alone does not repair an invalid
index. Keep both indexes through any image rollout; do not remove the
compatibility index until all deployed callers and rollback images are accounted
for. Production source images remain unchanged by this database mitigation.

```bash
docker compose --env-file ../.env ps
docker logs omni-indexer --since 10m | grep "Queue stats"      # Pending/Failed/Dead Letter
docker logs omni-connector-manager --since 10m | grep -i "circuit breaker"  # stuck sources
# Manual incremental sync (also resets the breaker on success): use the JSON
# /sync request in "Slack heartbeat rollout and verification" above.
```

Backups: `~/omni/backups/` (`docker exec omni-postgres pg_dump -U omni -Fc omni > file.fc`).
