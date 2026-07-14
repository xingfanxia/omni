# Omni Ops Runbook (cl-onyx production)

_Last verified: 2026-07-11 (upstream rebase deploy)._

## Layout

- Checkout: `~/omni/repo` (branch = whatever is deployed; 2026-07-11: `rebase/upstream-20260711`)
- Compose: run from `~/omni/repo/docker/` with `--env-file ../.env` (root `.env` is untracked; holds secrets + `OMNI_VERSION` + `ENABLED_CONNECTORS` profiles)
- `docker/docker-compose.override.yml` (untracked) pins locally-built custom images. Use immutable commit tags for new builds so rollback does not depend on a mutable `latest` tag.
- `OMNI_VERSION=latest` since 2026-07-11 — upstream stopped cutting semver at 0.1.2; each ghcr image's `latest` = its newest master build. Pull deliberately, never blind-restart onto a fresh pull.

## Build custom images (from repo root)

```bash
docker build -f services/<svc>/Dockerfile -t omni-<svc>-custom:latest .   # searcher/indexer/connector-manager/migrations(migrator)
docker build -f connectors/google/Dockerfile -t omni-google-connector-custom:latest .
docker build -f connectors/telegram/Dockerfile -t omni-telegram-connector:latest .
docker build -t omni-web-custom:latest web/                               # web builds with web/ context
```

For the bounded Slack-sync and content-blob-GC patch, build immutable images
from the exact deployed commit:

```bash
tag="$(git rev-parse --short=8 HEAD)"
docker build -f connectors/slack/Dockerfile -t "omni-slack-connector-custom:$tag" .
docker build -f services/indexer/Dockerfile -t "omni-indexer-custom:$tag" .
```

Pin both tags in `docker/docker-compose.override.yml`. Keep the Slack connector
at `mem_limit: 512m`; the connector rejects downloads above
`SLACK_MAX_DOWNLOAD_BYTES` (50 MiB by default) before buffering them.

The GC reconciliation queries are bounded and have transaction-local statement
and lock timeouts. The storage deletion path still has a small interval between
the database candidate transaction and object-store deletion, so a first deploy
must set `GC_DRY_RUN: "true"` on the indexer. Dry-run mode still marks and
unmarks references but does not delete objects. Trigger one run, confirm all
three phases finish within 30 seconds with sensible counts, then remove that
override only after an operator accepts enabling deletion:

```bash
docker run --rm --network docker_omni-network curlimages/curl -fsS \
  -X POST "http://indexer:${INDEXER_PORT}/admin/gc/run"
docker logs omni-indexer --since 10m | grep -E "garbage collection|GC completed|DRY RUN"
```

## Fork maintenance rules (learned the hard way)

1. **Migration checksums are sacred** — `_sqlx_migrations` checksums must match file bytes. Never renumber/edit an applied migration without a matching DB row fix.
2. **Constraint-rewrite migrations must be telegram-aware** — any upstream migration that rewrites `sources_source_type_check` / `service_credentials_provider_check` will fail on our data (telegram rows exist) unless patched to include `'telegram'`. Our newest telegram migration must stay the last rewrite and be a superset of upstream's latest list (see 105).
3. **pg_search 0.24.x query-shape limit** — `pdb.score()` + `JOIN sources` + `source_id = ANY($n)` + `LIMIT >= ~50` in one scan → `Unsupported query shape`. Searcher uses `ANY(ARRAY(SELECT id FROM sources WHERE NOT is_deleted))` instead of joins in bm25 CTEs, and MATERIALIZED fences. Re-test after any paradedb bump (repro: see commit 60719cf4).
4. **After a paradedb image bump**: check `datcollversion` vs OS glibc (`docker logs omni-postgres | grep collation`); if mismatched → `ALTER DATABASE ... REFRESH COLLATION VERSION` + `REINDEX DATABASE CONCURRENTLY omni`.

## Health checks

```bash
docker compose --env-file ../.env ps
docker logs omni-indexer --since 10m | grep "Queue stats"      # Pending/Failed/Dead Letter
docker logs omni-connector-manager --since 10m | grep -i "circuit breaker"  # stuck sources
# manual sync (also resets breaker on success):
docker run --rm --network docker_omni-network curlimages/curl -s -X POST -H "Content-Type: application/json" -d "{}" http://connector-manager:3004/sync/<source_id>
```

Backups: `~/omni/backups/` (`docker exec omni-postgres pg_dump -U omni -Fc omni > file.fc`).
