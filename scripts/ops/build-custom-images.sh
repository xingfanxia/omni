#!/usr/bin/env bash
# Build Omni custom images from rebase/upstream-20261108 at an exact commit.
set -uo pipefail
TAG=${1:?commit}
WT=~/omni/releases/$TAG
[ -d "$WT" ] || git -C ~/omni/repo worktree add --detach "$WT" "$TAG"
cd "$WT"
LOG=~/omni/build-$TAG.log
BUILDS=(
  "omni-migrator-custom services/migrations/Dockerfile ."
  "omni-indexer-custom services/indexer/Dockerfile ."
  "omni-searcher-custom services/searcher/Dockerfile ."
  "omni-connector-manager-custom services/connector-manager/Dockerfile ."
  "omni-slack-connector-custom connectors/slack/Dockerfile ."
  "omni-google-connector-custom connectors/google/Dockerfile ."
  "omni-web-connector-custom connectors/web/Dockerfile ."
  "omni-sandbox-custom services/sandbox/Dockerfile ."
  "omni-telegram-connector-custom connectors/telegram/Dockerfile ."
  "omni-notion-connector-custom connectors/notion/Dockerfile ."
  "omni-hubspot-connector-custom connectors/hubspot/Dockerfile ."
  "omni-ai-custom services/ai/Dockerfile services/ai"
  "omni-web-custom web/Dockerfile web"
)
{
echo "=== build start $(date -u) tag=$TAG ==="
for b in "${BUILDS[@]}"; do
  set -- $b
  echo "--- $1:$TAG $(date -u +%T) ---"
  if nice -n 10 docker build -q -f "$2" -t "$1:$TAG" "$3"; then echo "OK $1:$TAG"; else echo "FAILED $1:$TAG"; fi
done
echo "=== build end $(date -u) ==="
} >> "$LOG" 2>&1
