# Omni Agent-Facing Knowledge Platform

## Vision

Omni as the primary knowledge backend for AI agents (zylos/薄荷, OpenClaw, Claude Code) to access company internal information across Google Workspace, Notion, Slack, HubSpot, and Telegram.

## Current State (2026-04-06)

- **Production**: https://omni.computelabs.ai — GCE `cl-onyx` (n2-highmem-4, 32GB, us-west1-b)
- **Fork**: xingfanxia/omni — 4 bug fixes merged upstream (#109-#111), 5 PRs open (#112-#115, #119)
- **Indexing**: Gmail (32K), Google Drive (3.5K), Slack (3K), HubSpot (75K), Notion (5.9K) — **120K docs**
- **Search**: fulltext/semantic/hybrid via Postgres (ParadeDB BM25 + pgvector)
- **API**: Full agent API at `/api/v1/*` with three-level access control
- **MCP Server**: TypeScript, `@modelcontextprotocol/sdk`, 3 tools (omni_search, omni_document, omni_status)
- **Skills**: `omni-search`, `omni-doc`, `omni-status` deployed to all agents
- **Telegram**: Connector built with Telethon (full history) + Bot API (forward-only), not yet authed

## Architecture

```
Agent (薄荷/OpenClaw/Claude Code)
  ↓ curl via skill or MCP tool
  ↓ Authorization: Bearer omni_xxx
  ↓
Caddy (Let's Encrypt TLS at omni.computelabs.ai)
  ↓
SvelteKit Web App (custom image built from fork)
  ↓ hooks.server.ts: API key auth → rate limit → scope check
  ↓ locals.user + locals.apiKeyScope + locals.apiKeyAllowedSources
  ↓
  ├── /api/v1/search      → Searcher (Rust, BM25 + pgvector)
  ├── /api/v1/documents    → Searcher (document_id lookup)
  ├── /api/v1/sources      → Postgres (sources + sync_runs)
  ├── /api/v1/health       → All services (parallel health check)
  └── /api/v1/api-keys     → Postgres (api_keys CRUD)
  ↓
Postgres (ParadeDB 0.20.6) ← Indexer ← Connector Manager ← Connectors
                                    ├── Google (Gmail + Drive)
                                    ├── Slack
                                    ├── Notion
                                    ├── HubSpot
                                    └── Telegram
```

## GCE Deployment

**Host**: cl-onyx (n2-highmem-4, 4 vCPU, 32GB RAM, 1TB SSD)
**IP**: 34.187.165.174
**DNS**: omni.computelabs.ai → 34.187.165.174 (Cloudflare, DNS only)
**Local DNS**: `/etc/hosts` maps `omni.computelabs.ai` → `127.0.0.1` (agents on same host skip internet)
**TLS**: Caddy auto-provisions Let's Encrypt cert
**Replaced**: Onyx CE (removed, 108GB reclaimed)

### Resource usage (~4.5GB RAM total)

| Service | RAM | Notes |
|---------|-----|-------|
| Postgres (ParadeDB) | ~2.8 GB | shared_buffers=1GB, 13GB on disk |
| Searcher | ~570 MB | Rust, spikes during queries |
| Connector Manager | ~340 MB | Rust |
| AI Service | ~320 MB | Local embeddings |
| Google Connector | ~80 MB | |
| Notion/HubSpot/Slack | ~160 MB combined | |
| Web + Caddy + Redis | ~90 MB combined | |

### Deployment notes

- **Web image**: Must build custom from fork (`docker build -t omni-web-custom:latest web/`). Upstream images lack our API routes.
- **DB restore**: `pg_dump -Fc` with `2>/dev/null` to suppress docker compose stderr. Pipe into container via mounted volume, not `docker cp` (2GB+ files fail).
- **After restore**: BM25 indexes must be created manually (pg_restore can't recreate them). Run the CREATE INDEX from migration 057 with `mutable_segment_rows=5000` and `target_segment_count=2`.
- **Redis**: Set `system:flags.initialized = true` after restore (login state stored in Redis, not Postgres).
- **Chats table**: May need `ALTER TABLE chats ADD COLUMN agent_id TEXT` if the column is missing after restore.

---

## Agent Integrations

All agents use the `omni-search` skill (shell script that curls the API) with `OMNI_API_KEY` in their `.env`.

| Agent | Host | Skill Location | Repo |
|-------|------|----------------|------|
| 薄荷 (Zylos) | Local Mac | `~/.claude/skills/omni-*` | compute-labs-dev/zylos-core |
| 薄荷 (Zylos) | 136.109.155.206 | `~/.claude/skills/omni-*` | compute-labs-dev/zylos-core |
| OpenClaw | 34.187.165.174 | `~/.openclaw/workspace/skills/omni-search/` | xingfanxia/cl-oc-agent |
| OpenClaw | 136.109.155.206 | `~/.openclaw/workspace/skills/omni-search/` | xingfanxia/openclaw-config |
| Claude Code | Any | MCP server in `~/.claude/settings.json` | (global config) |

### MCP Server (Claude Code)

TypeScript MCP server at `omni-mcp-server/` in the repo. Globally configured in `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "omni": {
      "type": "stdio",
      "command": "npx",
      "args": ["tsx", "/path/to/omni-mcp-server/src/index.ts"],
      "env": {
        "OMNI_API_KEY": "omni_...",
        "OMNI_BASE_URL": "https://omni.computelabs.ai"
      }
    }
  }
}
```

3 tools: `omni_search`, `omni_document`, `omni_status`. All read-only.

---

## API Key Access Control

API keys have **two dimensions** of access control:

### Permission Scope (`scope`)

| Scope | What it sees | Who can create |
|-------|-------------|----------------|
| `public` (default) | Only docs where `permissions.public = true` | Any user |
| `user` | All docs the creating user has access to | Any user |
| `admin` | **All docs** — bypasses permission filters entirely | Admin role only |

### Source Scoping (`allowed_sources`)

| Config | Effect |
|--------|--------|
| `null` (default) | All sources |
| `["gmail", "slack"]` | Only those source types |

### Document Permission Model

| Source | Permission model |
|--------|-----------------|
| HubSpot | All public (CRM data shared across org) |
| Gmail | Per-user (your email → your threads only) |
| Google Drive | Per-file ACL (respects Google Drive sharing) |
| Slack | Channel members |
| Notion | Workspace group |

---

## BM25 Index Fix (migration 078)

**Root cause**: `mutable_segment_rows = 0` (migration 041) created a new immutable segment per batch write. Over time, 40+ segments accumulated, overwhelming the background merger. Concurrent queries reading stale segment metadata during failed merges caused `UUID parsing failed: invalid length: expected 16 bytes, found 0`.

**Fix**: `mutable_segment_rows = 5000` — buffers writes in RAM, flushes less frequently. After fix: 40 segments → 2 segments. Upstream PR #119.

**Not crash-related**: Container had 0 restarts, continuous uptime. This is a write-amplification issue, not a WAL/durability issue.

---

## Bug Fixes (contributed upstream)

### Merged
| PR | Fix |
|----|-----|
| #109 | Notion: missing `ServiceProvider` enum + API filter value |
| #110 | Multipart: missing `.mime_str()` on file upload → 400 on binary extraction |
| #111 | Google: Gmail attachment decode crash-loop + 100MB Drive file cap |

### Open
| PR | What |
|----|------|
| #112 | API key auth + source scoping + 3-level permission scope |
| #113 | Agent API endpoints with scope enforcement |
| #114 | Gmail batch 429 retry + adaptive backpressure + truncation retry + `-in:chats` filter + detailed logging |
| #115 | UI dedup context during sync |
| #119 | BM25 segment proliferation fix (mutable_segment_rows 0→5000) |

---

## Sync Schedule

| Source | Interval | Mode | How incremental works |
|--------|----------|------|----------------------|
| Gmail | 30 min | Incremental | `historyId` per user — only threads changed since last sync. Falls back to full if history expired (~30 days). Per-thread timestamp check skips unchanged. |
| Google Drive | 30 min | Incremental | Change tokens per user — only modified files since last sync |
| Slack | 30 min | Incremental | New messages since last sync |
| Notion | 60 min | Incremental | `last_edited_time` — only pages edited since last sync |
| HubSpot | 60 min | Incremental | Re-checks all records for updates |

**Gmail deduplication**: Same thread appears for sender + all recipients. The connector tracks processed thread IDs across users and only indexes each thread once.

---

## Next Steps

| Priority | Task | Status |
|----------|------|--------|
| P1 | Telegram: get api_id/api_hash, run auth, select chats | TODO |
| P2 | Wait for upstream to review/merge #112-#115, #119 | Open |
| P3 | Iterate on search quality based on agent usage | Ongoing |
