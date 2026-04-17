#!/usr/bin/env bash
# Cleanup orphaned content_blobs on prod Omni postgres.
#
# The indexer's GC path runs row-by-row inside the main tokio::select! loop,
# which blocks on 8M+ orphans and stalls event processing. This script does
# the cleanup externally in batched transactions so the indexer can be
# restarted with a clean slate.
#
# Usage:
#   DRY_RUN=1 ./cleanup-orphan-blobs.sh     # counts only
#   ./cleanup-orphan-blobs.sh               # stops indexer, cleans, restarts
#
# Env:
#   SSH_HOST        remote ssh target         (default: xingfanxia@34.187.165.174)
#   BATCH_SIZE      rows per DELETE           (default: 10000)
#   RETENTION_DAYS  orphaned_at cutoff        (default: 7)
#   DRY_RUN         1 = skip indexer stop/delete/restart

set -euo pipefail

SSH_HOST="${SSH_HOST:-xingfanxia@34.187.165.174}"
BATCH_SIZE="${BATCH_SIZE:-10000}"
RETENTION_DAYS="${RETENTION_DAYS:-7}"
DRY_RUN="${DRY_RUN:-0}"

psql_q() {
  ssh "$SSH_HOST" "docker exec omni-postgres psql -U omni -d omni -tAc \"$1\""
}

section() { printf "\n=== %s ===\n" "$1"; }

section "Before"
psql_q "SELECT 'total blobs: ' || count(*) FROM content_blobs;"
psql_q "SELECT 'eligible orphans: ' || count(*) || ', size: ' || pg_size_pretty(COALESCE(sum(size_bytes), 0)) FROM content_blobs WHERE orphaned_at IS NOT NULL AND orphaned_at < now() - interval '${RETENTION_DAYS} days';"
psql_q "SELECT 'table size: ' || pg_size_pretty(pg_total_relation_size('content_blobs'));"

if [[ "$DRY_RUN" == "1" ]]; then
  echo ""
  echo "DRY_RUN=1 — no changes made"
  exit 0
fi

section "Stopping indexer"
ssh "$SSH_HOST" "docker stop omni-indexer" >/dev/null
echo "indexer stopped"

section "Bulk delete (batch=${BATCH_SIZE})"
total=0
start=$(date +%s)
while true; do
  rc=$(ssh "$SSH_HOST" "docker exec omni-postgres psql -U omni -d omni -tAc \"WITH victims AS (SELECT id FROM content_blobs WHERE orphaned_at IS NOT NULL AND orphaned_at < now() - interval '${RETENTION_DAYS} days' LIMIT ${BATCH_SIZE}) DELETE FROM content_blobs cb USING victims WHERE cb.id = victims.id RETURNING 1;\" | wc -l | tr -d ' ')

  [[ -z "$rc" || "$rc" == "0" ]] && break

  total=$((total + rc))
  elapsed=$(( $(date +%s) - start ))
  rate=$(( total / (elapsed + 1) ))
  remaining=$(( 8055204 - total ))
  eta=$(( remaining / (rate + 1) ))
  printf "[%s] batch=%s total=%s elapsed=%ss rate=%s/s eta=%ss\n" \
    "$(date +%H:%M:%S)" "$rc" "$total" "$elapsed" "$rate" "$eta"
done

section "VACUUM"
psql_q "VACUUM content_blobs;" || echo "(vacuum warnings ignored)"

section "After"
psql_q "SELECT 'total blobs: ' || count(*) FROM content_blobs;"
psql_q "SELECT 'orphans remaining: ' || count(*) FROM content_blobs WHERE orphaned_at IS NOT NULL;"
psql_q "SELECT 'table size: ' || pg_size_pretty(pg_total_relation_size('content_blobs'));"

section "Restarting indexer"
ssh "$SSH_HOST" "docker start omni-indexer" >/dev/null
echo "indexer started"

section "Done"
