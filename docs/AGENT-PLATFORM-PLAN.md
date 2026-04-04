# Omni Agent-Facing Knowledge Platform — Implementation Plan

## Vision

Omni as the primary knowledge backend for AI agents (zylos/薄荷 and others) to access company internal information across Google Workspace, Notion, Slack, HubSpot, and Telegram.

## Current State (v0.1.2 + our enhancements)

- Indexing: Gmail, Google Drive, Slack, HubSpot, Notion all syncing (82K+ docs)
- Search: fulltext/semantic/hybrid via Postgres (ParadeDB + pgvector), <1s latency
- **API Key Auth: ✅ Implemented** — `Authorization: Bearer omni_*` or `X-API-Key` header
- **Agent API: ✅ Implemented** — `/api/v1/search`, `/api/v1/documents/:id`, `/api/v1/health`, `/api/v1/sources`, `/api/v1/api-keys`
- **Telegram Connector: ✅ Implemented** — Python connector using Bot API
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

## API Reference

### Authentication

All `/api/v1/*` endpoints (except `/api/v1/health`) require authentication via:
- `Authorization: Bearer omni_<key>` header
- `X-API-Key: omni_<key>` header

API keys are managed via `/api/v1/api-keys` (create, list, revoke, delete).

Rate limit: 30 requests/60s per IP on API key auth attempts.

### Endpoints

#### `POST /api/v1/search`

Search across all indexed sources.

**Request:**
```json
{
  "query": "quarterly revenue report",
  "source_types": ["google_drive", "slack"],
  "content_types": ["document", "message"],
  "mode": "hybrid",
  "limit": 20,
  "offset": 0
}
```

**Response:**
```json
{
  "results": [
    {
      "document": {
        "id": "01ABC...",
        "title": "Q4 Revenue Report",
        "url": "https://docs.google.com/...",
        "content_type": "document",
        "metadata": { "author": "...", "created_at": "...", "updated_at": "..." }
      },
      "score": 0.87,
      "highlights": ["...matched text **snippet**..."],
      "match_type": "hybrid"
    }
  ],
  "total_count": 42,
  "query_time_ms": 120,
  "has_more": true
}
```

#### `GET /api/v1/documents/:id`

Fetch full document content.

**Query params:** `?start_line=100&end_line=200` for large document line ranges.

**Response:**
```json
{
  "id": "01ABC...",
  "title": "Meeting Notes",
  "url": "https://...",
  "source_type": "google_drive",
  "content_type": "document",
  "content": "Full document text...",
  "match_type": "full_content",
  "metadata": {},
  "created_at": "...",
  "updated_at": "..."
}
```

#### `GET /api/v1/health`

Service health check (no auth required).

**Response:**
```json
{
  "status": "healthy",
  "services": {
    "postgres": { "status": "ok", "latency_ms": 5 },
    "redis": { "status": "ok", "latency_ms": 2 },
    "searcher": { "status": "ok", "latency_ms": 3 },
    "indexer": { "status": "ok", "latency_ms": 4 },
    "connector_manager": { "status": "ok", "latency_ms": 2 }
  },
  "timestamp": "2026-04-03T18:00:00.000Z"
}
```

#### `GET /api/v1/sources`

List all data sources with sync status and document counts.

**Response:**
```json
{
  "sources": [
    {
      "id": "...",
      "name": "Gmail",
      "source_type": "gmail",
      "is_active": true,
      "is_connected": true,
      "document_count": 20000,
      "sync_status": "completed",
      "last_sync_at": "2026-04-03T17:00:00Z",
      "documents_scanned": 20854,
      "documents_processed": 20000,
      "sync_error": null,
      "created_at": "..."
    }
  ]
}
```

#### `POST /api/v1/api-keys` — Create key
#### `GET /api/v1/api-keys` — List keys
#### `PATCH /api/v1/api-keys` — Revoke key (`{"id": "...", "action": "revoke"}`)
#### `DELETE /api/v1/api-keys` — Delete key (`{"id": "..."}`)

## Upstream PR Plan (small independent PRs)

### PR #1: Notion Fix — ✅ MERGED (upstream PR #109)
- Fixed `ServiceProvider` enum missing NOTION
- Fixed Notion API `search_databases` filter value
- Fixed `APIResponseError` attribute access

### PR #2: API Key Authentication — ✅ IMPLEMENTED
**Files:**
- `services/migrations/075_create_api_keys_table.sql`
- `web/src/lib/server/db/schema.ts` — Drizzle schema for api_keys
- `web/src/lib/server/apiKeys.ts` — Key generation (SHA256 + timing-safe), validation, CRUD
- `web/src/hooks.server.ts` — API key auth before cookie auth, rate limiting
- `web/src/routes/api/v1/api-keys/+server.ts` — CRUD endpoints

**Security:** Rate limiting (30/60s), timing-safe hash comparison, per-user key cap (25), mustChangePassword enforcement, key prefix for display without leaking full key.

### PR #3: Agent Search API — ✅ IMPLEMENTED
**Files:**
- `web/src/routes/api/v1/search/+server.ts`

### PR #4: Document Content API — ✅ IMPLEMENTED
**Files:**
- `web/src/routes/api/v1/documents/[id]/+server.ts`

### PR #5: Health & Sync Status API — ✅ IMPLEMENTED
**Files:**
- `web/src/routes/api/v1/health/+server.ts`
- `web/src/routes/api/v1/sources/+server.ts`

### PR #6: Telegram Connector — ✅ IMPLEMENTED
**Files:**
- `connectors/telegram/` — Full Python connector package
- `docker/docker-compose.yml` — Telegram service entry
- `web/src/lib/types.ts` — TELEGRAM enum values

## Skill Design (local, not upstream)

After deployment, build Claude Code skills:

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

## Key Technical Findings

### Searcher already supports everything we need:
- Fulltext (BM25), semantic (pgvector), hybrid (RRF) search modes
- Full document retrieval via `document_id` parameter
- Line-range retrieval for large documents
- Permission filtering via `user_email`
- Content returned in `highlights` field for full_content match_type
- Highlights use simple `**bold**` markdown markers

### No Rust changes needed:
- All API key auth and routing lives in the SvelteKit web layer
- Searcher is an internal service with no auth — web app handles everything
- Existing proxy pattern (inject user_email → forward to searcher) works perfectly

### Content format is already clean:
- Connectors extract plaintext during indexing
- Content stored via content_storage, referenced by content_id
- documents.content has denormalized plaintext for BM25 indexing
- No HTML stripping or format conversion needed

## Deployment Plan

1. **Local** (current): Docker Compose on Mac, all connectors syncing ✅
2. **GCP**: pg_dump local DB → pg_restore on GCP Cloud SQL or GCE Postgres
3. **Production**: Same Docker Compose on GCE instance, Caddy handles TLS

## Next Steps

| Priority | Task | Status |
|----------|------|--------|
| P0 | Build omni-search/omni-doc/omni-status skills | TODO |
| P0 | Create production API key for 薄荷 | TODO |
| P1 | GCP deployment (pg_dump → restore) | TODO |
| P1 | Submit PRs #2-#6 upstream | TODO |
| P2 | Telegram bot token setup + test sync | TODO |
| P3 | Iterate based on agent usage patterns | Ongoing |
