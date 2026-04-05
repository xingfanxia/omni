# Omni Agent-Facing Knowledge Platform

## Vision

Omni as the primary knowledge backend for AI agents (zylos/薄荷 and others) to access company internal information across Google Workspace, Notion, Slack, HubSpot, and Telegram.

## Current State (2026-04-05)

- **Fork**: xingfanxia/omni — 4 bug fixes merged upstream (#109, #110, #111), 2 feature PRs open (#112, #113)
- **Indexing**: Gmail (32K), Google Drive (3.5K), Slack (3K), HubSpot (75K), Notion (5.9K) — **120K docs**
- **Search**: fulltext/semantic/hybrid via Postgres (ParadeDB BM25 + pgvector), <300ms
- **API**: Full agent API at `/api/v1/*` with three-level access control
- **Skills**: `omni-search`, `omni-doc`, `omni-status` installed at `~/.claude/skills/`
- **Telegram**: Connector built with Telethon (full history) + Bot API (forward-only)

## Architecture

```
Agent (薄荷/Claude Code)
  ↓ curl via skill (omni-search / omni-doc / omni-status)
  ↓ Authorization: Bearer omni_xxx
  ↓
SvelteKit Web App (Caddy reverse proxy, HTTPS)
  ↓ hooks.server.ts: API key auth → rate limit → scope check
  ↓ locals.user + locals.apiKeyScope + locals.apiKeyAllowedSources
  ↓
  ├── /api/v1/search      → Searcher (Rust, BM25 + pgvector)
  ├── /api/v1/documents    → Searcher (document_id lookup)
  ├── /api/v1/sources      → Postgres (sources + sync_runs)
  ├── /api/v1/health       → All services (parallel health check)
  └── /api/v1/api-keys     → Postgres (api_keys CRUD)
  ↓
Postgres (ParadeDB) ← Indexer ← Connector Manager ← Connectors
                                    ├── Google (Gmail + Drive)
                                    ├── Slack
                                    ├── Notion
                                    ├── HubSpot
                                    └── Telegram
```

---

## API Key Access Control

API keys have **two dimensions** of access control:

### Permission Scope (`scope`)

Controls which documents a key can see based on document-level permissions:

| Scope | What it sees | Who can create |
|-------|-------------|----------------|
| `public` (default) | Only docs where `permissions.public = true` | Any user |
| `user` | All docs the creating user has access to | Any user |
| `admin` | **All docs** — bypasses permission filters entirely | Admin role only |

**How it works**: The scope controls what `user_email` is sent to the searcher:
- `public` → sentinel email `__public_access__@omni.internal` → permission filter only matches public docs
- `user` → creating user's real email → normal permission filtering
- `admin` → no `user_email` → searcher skips permission filter

### Source Scoping (`allowed_sources`)

Controls which connectors/source types a key can query:

| Config | Effect |
|--------|--------|
| `null` (default) | All sources — gmail, google_drive, slack, notion, hubspot, telegram |
| `["gmail", "slack"]` | Only Gmail and Slack results. Other sources hidden from search, docs, and sources listing |

**Enforcement points**:
- **Search**: `source_types` intersected with `allowed_sources` before sending to searcher
- **Documents**: `source_type` checked, returns 403 if not in `allowed_sources`
- **Sources listing**: filtered to only show allowed source types

### Examples

```bash
# Public key — safest for sharing (only public docs, all sources)
POST /api/v1/api-keys
{"name": "shared-agent", "scope": "public"}

# Personal agent — sees your docs (emails, shared files, etc.)
POST /api/v1/api-keys
{"name": "my-agent", "scope": "user"}

# Admin key — full access to everything
POST /api/v1/api-keys
{"name": "admin-agent", "scope": "admin"}

# Scoped key — only Gmail + Drive, no Telegram/Slack/etc.
POST /api/v1/api-keys
{"name": "limited", "scope": "user", "allowed_sources": ["gmail", "google_drive"]}
```

### Document Permission Model

Each connector sets document permissions during indexing:

| Source | Permission model | Details |
|--------|-----------------|---------|
| HubSpot | All public | CRM data shared across org (75K docs) |
| Gmail | Per-user | Your email → your threads only (27K docs) |
| Google Drive | Per-file ACL | Respects Google Drive sharing settings (3.5K docs) |
| Slack | Channel members | Public channel members listed as permitted users (3K docs) |
| Notion | Workspace group | All workspace members via group permission (5.9K docs) |

---

## API Reference

### Authentication

All `/api/v1/*` endpoints (except `/api/v1/health`) require:
- `Authorization: Bearer omni_<key>` header, OR
- `X-API-Key: omni_<key>` header

Rate limit: 30 attempts per 60 seconds per IP on failed API key validation.

### `POST /api/v1/search`

```json
// Request
{
  "query": "quarterly revenue report",
  "source_types": ["google_drive", "slack"],
  "mode": "hybrid",
  "limit": 20,
  "offset": 0
}

// Response
{
  "results": [{
    "document": { "id": "01ABC...", "title": "Q4 Report", "url": "https://..." },
    "score": 0.87,
    "highlights": ["...matched **text**..."],
    "match_type": "hybrid"
  }],
  "total_count": 42,
  "query_time_ms": 120,
  "has_more": true
}
```

Modes: `hybrid` (default, best for agents), `fulltext` (BM25), `semantic` (pgvector).
Limit capped at 100. Source scoping enforced per API key.

### `GET /api/v1/documents/:id`

Returns full document content. Use `?start_line=N&end_line=M` for large docs.

```json
{
  "id": "01ABC...",
  "title": "Meeting Notes",
  "content": "Full document text (can be 10K+ chars)...",
  "source_type": "google_drive",
  "match_type": "full_content",
  "metadata": { "author": "...", "url": "https://..." }
}
```

### `GET /api/v1/health`

No auth required. Returns 200 if all healthy, 503 if degraded.

### `GET /api/v1/sources`

Lists sources with document counts and sync status. Filtered by API key's `allowed_sources`.

### `POST /api/v1/api-keys` — Create key

```json
{"name": "agent-name", "scope": "user", "allowed_sources": ["gmail"], "expires_at": "2027-01-01T00:00:00Z"}
```

### `GET /api/v1/api-keys` — List keys
### `PATCH /api/v1/api-keys` — Revoke (`{"id": "...", "action": "revoke"}`)
### `DELETE /api/v1/api-keys` — Delete (`{"id": "..."}`)

---

## Telegram Connector

### Two backends

| Backend | Auth | History | Use case |
|---------|------|---------|----------|
| **Telethon (MTProto)** | User session (phone + api_id + api_hash) | Full chat history | Primary — index everything |
| **Bot API** | Bot token | Forward-only (new messages) | Lightweight — ongoing capture |

Auto-detected from credentials: `session` + `api_id` + `api_hash` → Telethon, `token` → Bot API.

### Setup: Telethon (recommended)

**Step 1**: Get API credentials from https://my.telegram.org/apps → copy `api_id` and `api_hash`

**Step 2**: Generate session string (one-time, run locally):
```bash
cd connectors/telegram
pip install telethon
python scripts/auth.py --api-id YOUR_API_ID --api-hash YOUR_API_HASH
```
This prompts for phone number + verification code, outputs a session string.

**Step 3**: In Omni, create a Telegram source with credentials:
```json
{
  "api_id": "12345",
  "api_hash": "abc123def456...",
  "session": "1BVtsOHk..."
}
```

**Step 4**: Select chats to sync. Use the `list_chats` action to see all available chats:
```
Action: list_chats → returns [{"id": -100123, "title": "Team Chat", "type": "supergroup", ...}, ...]
```

Then set source config with chat names:
```json
{"chats": ["Compute Labs Internal", "GPU Deals Discussion"]}
```

Leave `chats` empty to sync ALL dialogs.

**Step 5**: Trigger sync. First run fetches full history, subsequent runs are incremental (only new messages since `last_message_id`).

### Setup: Bot API (forward-only)

Create a bot via [@BotFather](https://t.me/BotFather), add it to chats, use credentials:
```json
{"token": "123456:ABC-DEF..."}
```

### Docker

```bash
# Add to .env
TELEGRAM_CONNECTOR_PORT=8010

# Start with telegram profile
docker compose --profile telegram up -d
```

---

## Bug Fixes (contributed upstream)

### Merged
| PR | Fix |
|----|-----|
| #109 | Notion: missing `ServiceProvider` enum + API filter value + error attribute |
| #110 | Multipart: missing `.mime_str()` on Rust SDK file upload → 400 on binary extraction |
| #111 | Google: Gmail attachment decode crash-loop (5x retry on deterministic failure) + 100MB Drive file cap |

### Open
| PR | Feature |
|----|---------|
| #112 | API key auth + source scoping + 3-level permission scope (public/user/admin) |
| #113 | Agent API endpoints (/api/v1/search, documents, health, sources) with scope enforcement |

### Fork-only fixes (not yet PR'd)
| Fix | What |
|-----|------|
| Gmail batch 429 retry | Per-thread 429s inside batch responses are retried with exponential backoff instead of silently dropped |
| Adaptive backpressure | Pauses 3s before next batch when 429s detected, reducing rate limit hits proactively |
| Batch truncation retry | Threads missing from truncated batch responses are retried instead of lost |
| Connector-manager body limit | Raised `/sdk/extract-content` from 2MB default to 100MB for large PDFs |
| UI dedup context | Sync status shows "(X indexed, scanned includes duplicates across users)" |
| Detailed sync logging | Per-user breakdown: listed, indexed, updated, deduped, unchanged, failed |

---

## Claude Code Skills

Three skills installed at `~/.claude/skills/` (inherited by all agents including 薄荷):

| Skill | Command | What |
|-------|---------|------|
| `omni-search` | `curl /api/v1/search` | Search across all indexed sources |
| `omni-doc` | `curl /api/v1/documents/:id` | Read full document content |
| `omni-status` | `curl /api/v1/health` + `/sources` | Health check + sync status |

Environment: `OMNI_API_KEY` must be set (in `~/zylos/.env` for 薄荷).

---

## Sync Schedule

| Source | Interval | Mode | How incremental works |
|--------|----------|------|----------------------|
| Gmail | 30 min | Incremental | `historyId` per user — only threads changed since last sync. Falls back to full if history expired (~30 days). Per-thread timestamp check skips unchanged. |
| Google Drive | 30 min | Incremental | Change tokens per user — only modified files since last sync |
| Slack | 30 min | Incremental | New messages since last sync |
| Notion | 60 min | Incremental | `last_edited_time` — only pages edited since last sync |
| HubSpot | 60 min | Incremental | Re-checks all records for updates |

**Gmail deduplication**: Same thread appears for sender + all recipients. The connector tracks processed thread IDs across users and only indexes each thread once. This is why "scanned" (sum of all users' thread lists) > "indexed" (unique threads).

---

## Deployment

1. **Local** (current): Docker Compose on Mac ✅
2. **GCP**: `pg_dump` local → `pg_restore` on GCP (Cloud SQL or GCE Postgres)
3. **Production**: Same Docker Compose on GCE, Caddy handles TLS

---

## Next Steps

| Priority | Task | Status |
|----------|------|--------|
| P0 | Restart 薄荷 to pick up `OMNI_API_KEY` from `~/zylos/.env` | TODO |
| P1 | GCP deployment | TODO |
| P1 | Telegram: get api_id/api_hash, run auth, select chats | TODO |
| P1 | PR fork-only fixes upstream (batch 429 retry, backpressure, UI) | TODO |
| P2 | Wait for upstream to review/merge #112 + #113 | Open |
| P3 | Iterate on search quality based on agent usage | Ongoing |
