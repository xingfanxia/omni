# Omni Agent-Facing Knowledge Platform — Implementation Plan

## Vision

Omni as the primary knowledge backend for AI agents (zylos/薄荷 and others) to access company internal information across Google Workspace, Notion, Slack, HubSpot, and Telegram.

## Current State (v0.1.2 + our fixes)

- Indexing: Gmail, Google Drive, Slack, HubSpot, Notion all syncing
- Search: fulltext/semantic/hybrid via Postgres (ParadeDB + pgvector), <1s latency
- Auth: session cookie only — agents can't authenticate
- No public API documentation
- Our fork: xingfanxia/omni (2 bug fixes merged upstream)

## Architecture Overview

```
Agent (薄荷/Claude Code)
  ↓ skill (curl/fetch)
  ↓ Authorization: Bearer omni_xxx
SvelteKit Web App (port 80 via Caddy)
  ↓ hooks.server.ts validates API key → populates locals.user
  ↓ /api/v1/* routes inject user_email/user_id
Searcher Service (Rust, internal port 3001)
  ↓ BM25 fulltext + pgvector semantic
Postgres (ParadeDB)
```

## Upstream PR Plan (small independent PRs)

### PR #2: API Key Authentication

**Files:**
- `services/migrations/075_create_api_keys_table.sql` — new table
- `web/src/lib/server/db/schema.ts` — Drizzle schema for api_keys
- `web/src/lib/server/auth.ts` — add `validateApiKey()` function
- `web/src/hooks.server.ts` — check `Authorization: Bearer omni_*` or `X-API-Key` header before cookie auth
- `web/src/routes/api/api-keys/+server.ts` — CRUD endpoints for key management
- `web/src/routes/(admin)/admin/settings/api-keys/+page.svelte` — UI to manage keys

**DB Schema:**
```sql
CREATE TABLE api_keys (
    id CHAR(26) PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    key_hash CHAR(64) NOT NULL,       -- SHA256 of full key
    key_prefix VARCHAR(12) NOT NULL,  -- "omni_abc1" for display
    name TEXT NOT NULL,
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_api_keys_hash ON api_keys(key_hash);
```

**Auth flow:**
1. `hooks.server.ts` checks `Authorization` or `X-API-Key` header
2. SHA256 hash the key, lookup in api_keys table
3. Load associated user, populate `locals.user`
4. Fall through to existing cookie auth if no API key header

**Size:** ~6 files, well-scoped

---

### PR #3: Agent Search API (`/api/v1/search`)

**Depends on:** PR #2 (API key auth)

**Files:**
- `web/src/routes/api/v1/search/+server.ts`

**Behavior:**
- `POST /api/v1/search` with JSON body
- Requires API key auth
- Injects `user_email` and `user_id` from authenticated user
- Proxies to searcher's `/search` endpoint
- Returns clean JSON response

**Request:**
```json
{
  "query": "quarterly revenue report",
  "source_types": ["google_drive", "slack"],
  "mode": "hybrid",
  "limit": 20
}
```

**Response:**
```json
{
  "results": [
    {
      "title": "Q4 Revenue Report",
      "url": "https://docs.google.com/...",
      "source_type": "google_drive",
      "score": 0.87,
      "content": "Full or partial document content...",
      "highlights": ["...matched text **snippet**..."],
      "metadata": { "author": "...", "created_at": "...", "updated_at": "..." }
    }
  ],
  "total_count": 42,
  "query_time_ms": 120,
  "has_more": true
}
```

**Size:** ~1 file, tiny PR

---

### PR #4: Document Content API (`/api/v1/documents/:id`)

**Depends on:** PR #2

**Files:**
- `web/src/routes/api/v1/documents/[id]/+server.ts`

**Behavior:**
- `GET /api/v1/documents/:id` — full document content
- `GET /api/v1/documents/:id?start_line=100&end_line=200` — line range for large docs
- Uses searcher's existing `document_id` field on SearchRequest
- Documents <50KB: returns full content
- Documents ≥50KB: returns requested line range or first 500 lines

**Size:** ~1 file

---

### PR #5: Health & Sync Status API

**Depends on:** PR #2

**Files:**
- `web/src/routes/api/v1/health/+server.ts`
- `web/src/routes/api/v1/sources/+server.ts`

**Endpoints:**
- `GET /api/v1/health` — aggregated service health (searcher, indexer, connector-manager, Redis, Postgres)
- `GET /api/v1/sources` — list sources with sync status, document counts, last sync time

**Size:** ~2 files

---

### PR #6: Telegram Connector (separate, no dependency)

**Files:** New `connectors/telegram/` directory

**Approach:**
- Python connector using SDK (`omni-connector` package)
- Telegram Bot API or MTProto (via Telethon) for chat history
- Full/incremental sync modes
- Index: messages, media captions, file attachments
- Permissions: public by default (private bot chats)

**Effort:** ~2-3 days
**Size:** New connector package, independent PR

---

## Skill Design (local, not upstream)

After PR #2 + #3 land, build Claude Code skills:

```
~/.claude/skills/omni-search/
├── SKILL.md           # Search company knowledge base
└── scripts/
    └── search.sh      # curl wrapper

~/.claude/skills/omni-doc/
├── SKILL.md           # Read full document content
└── scripts/
    └── read.sh        # curl wrapper

~/.claude/skills/omni-status/
├── SKILL.md           # Check health and sync status
└── scripts/
    └── status.sh      # curl wrapper
```

Each skill is a thin wrapper around `curl` to the `/api/v1/*` endpoints with the API key from env.

## Key Technical Findings

### Searcher already supports everything we need:
- Fulltext (BM25), semantic (pgvector), hybrid (RRF) search modes
- Full document retrieval via `document_id` parameter
- Line-range retrieval for large documents
- Permission filtering via `user_email`
- All content stored as plaintext (not HTML) — already agent-friendly
- Highlights use simple `**bold**` markdown markers

### No Rust changes needed:
- All API key auth and routing can live in the SvelteKit web layer
- Searcher is an internal service with no auth — web app handles everything
- Existing proxy pattern (inject user_email → forward to searcher) works perfectly

### Content format is already clean:
- Connectors extract plaintext during indexing
- content_blobs stores raw bytes, mostly text/plain
- documents.content has denormalized plaintext for BM25 indexing
- No HTML stripping or format conversion needed

## Deployment Plan

1. **Local** (current): Docker Compose on Mac, verify all connectors + search quality
2. **GCP**: pg_dump local DB → pg_restore on GCP Cloud SQL or GCE Postgres
3. **Production**: Same Docker Compose on GCE instance, Caddy handles TLS

## Timeline

| Week | Deliverable |
|------|-------------|
| Now | Local validation, indexing all sources |
| W1 | PR #2 (API key auth) + PR #3 (search API) |
| W1 | Build omni-search skill, 薄荷 can query |
| W2 | PR #4 (doc content) + PR #5 (health) |
| W2 | GCP deployment |
| W3 | PR #6 (Telegram connector) |
| Ongoing | Iterate based on agent usage patterns |
