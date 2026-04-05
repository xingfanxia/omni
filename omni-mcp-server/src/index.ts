import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import https from "node:https";

// ---------------------------------------------------------------------------
// Startup validation
// ---------------------------------------------------------------------------

const OMNI_API_KEY = process.env.OMNI_API_KEY;
if (!OMNI_API_KEY) {
  console.error(
    "Fatal: OMNI_API_KEY environment variable is required but not set."
  );
  process.exit(1);
}

const BASE_URL = process.env.OMNI_BASE_URL ?? "https://localhost";

// ---------------------------------------------------------------------------
// HTTP client helpers
// ---------------------------------------------------------------------------

// Accept self-signed certs (set via NODE_TLS_REJECT_UNAUTHORIZED=0 or custom agent)
const tlsAgent = new https.Agent({ rejectUnauthorized: false });

async function omniGet(path: string, auth = true): Promise<unknown> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
  };
  if (auth) {
    headers["Authorization"] = `Bearer ${OMNI_API_KEY}`;
  }

  const res = await fetch(`${BASE_URL}${path}`, {
    method: "GET",
    headers,
    // @ts-expect-error — node-fetch / undici accept agent, native fetch typing doesn't expose it
    agent: tlsAgent,
  });

  if (!res.ok) {
    const body = await res.text().catch(() => "(no body)");
    throw new Error(`Omni API error ${res.status} ${res.statusText}: ${body}`);
  }

  return res.json();
}

async function omniPost(path: string, body: unknown): Promise<unknown> {
  const res = await fetch(`${BASE_URL}${path}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${OMNI_API_KEY}`,
    },
    body: JSON.stringify(body),
    // @ts-expect-error — same as above
    agent: tlsAgent,
  });

  if (!res.ok) {
    const text = await res.text().catch(() => "(no body)");
    throw new Error(`Omni API error ${res.status} ${res.statusText}: ${text}`);
  }

  return res.json();
}

// ---------------------------------------------------------------------------
// Response formatters
// ---------------------------------------------------------------------------

interface SearchResult {
  document: { id: string; title: string; url?: string };
  score: number;
  highlights: string[];
  match_type: string;
}

interface SearchResponse {
  results: SearchResult[];
  total_count: number;
  query_time_ms: number;
  has_more: boolean;
}

function formatSearchResults(data: SearchResponse, query: string): string {
  const { results, total_count, query_time_ms, has_more } = data;

  if (results.length === 0) {
    return `No results found for query: **${query}**`;
  }

  const lines: string[] = [
    `## Search Results for "${query}"`,
    `Found **${total_count}** result(s) (${query_time_ms}ms)${has_more ? " — more results available" : ""}`,
    "",
  ];

  for (let i = 0; i < results.length; i++) {
    const r = results[i];
    lines.push(`### ${i + 1}. ${r.document.title}`);
    lines.push(`- **ID**: \`${r.document.id}\``);
    if (r.document.url) lines.push(`- **URL**: ${r.document.url}`);
    lines.push(`- **Score**: ${r.score.toFixed(4)} (${r.match_type})`);
    if (r.highlights.length > 0) {
      lines.push("- **Highlights**:");
      for (const h of r.highlights) {
        lines.push(`  > ${h.trim()}`);
      }
    }
    lines.push("");
  }

  return lines.join("\n");
}

interface DocumentResponse {
  id: string;
  title: string;
  url?: string;
  source_type?: string;
  content_type?: string;
  content?: string;
  match_type?: string;
  metadata?: Record<string, unknown>;
  created_at?: string;
  updated_at?: string;
}

function formatDocument(doc: DocumentResponse): string {
  const lines: string[] = [
    `# ${doc.title}`,
    "",
    "## Metadata",
    `| Field | Value |`,
    `|-------|-------|`,
    `| ID | \`${doc.id}\` |`,
  ];

  if (doc.url) lines.push(`| URL | ${doc.url} |`);
  if (doc.source_type) lines.push(`| Source | ${doc.source_type} |`);
  if (doc.content_type) lines.push(`| Content Type | ${doc.content_type} |`);
  if (doc.match_type) lines.push(`| Match Type | ${doc.match_type} |`);
  if (doc.created_at) lines.push(`| Created | ${doc.created_at} |`);
  if (doc.updated_at) lines.push(`| Updated | ${doc.updated_at} |`);

  if (doc.metadata && Object.keys(doc.metadata).length > 0) {
    lines.push("", "## Extra Metadata");
    for (const [k, v] of Object.entries(doc.metadata)) {
      lines.push(`- **${k}**: ${JSON.stringify(v)}`);
    }
  }

  if (doc.content) {
    lines.push("", "## Content", "", doc.content);
  }

  return lines.join("\n");
}

interface HealthResponse {
  status: string;
  [key: string]: unknown;
}

interface Source {
  name?: string;
  source_type?: string;
  status?: string;
  last_synced_at?: string;
  document_count?: number;
  [key: string]: unknown;
}

interface SourcesResponse {
  sources?: Source[];
  [key: string]: unknown;
}

function formatStatus(
  health: HealthResponse,
  sources: SourcesResponse | null
): string {
  const lines: string[] = [
    "## Omni Platform Status",
    "",
    `**Overall Health**: ${health.status}`,
  ];

  // Any extra fields from health endpoint
  const healthExtras = Object.entries(health).filter(([k]) => k !== "status");
  if (healthExtras.length > 0) {
    lines.push("");
    for (const [k, v] of healthExtras) {
      lines.push(`- **${k}**: ${JSON.stringify(v)}`);
    }
  }

  if (sources?.sources && sources.sources.length > 0) {
    lines.push("", "## Sources", "");
    lines.push(
      "| Source | Type | Status | Last Synced | Documents |"
    );
    lines.push("|--------|------|--------|-------------|-----------|");

    for (const s of sources.sources) {
      const name = s.name ?? s.source_type ?? "(unknown)";
      const type = s.source_type ?? "—";
      const status = s.status ?? "—";
      const synced = s.last_synced_at ?? "—";
      const count = s.document_count != null ? String(s.document_count) : "—";
      lines.push(`| ${name} | ${type} | ${status} | ${synced} | ${count} |`);
    }
  } else if (sources === null) {
    lines.push("", "_Sources information unavailable._");
  } else {
    lines.push("", "_No sources configured._");
  }

  return lines.join("\n");
}

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

const server = new McpServer({
  name: "omni",
  version: "1.0.0",
});

// --- Tool: omni_search ---

server.registerTool(
  "omni_search",
  {
    title: "Omni Search",
    description:
      "Search the Omni knowledge platform using hybrid, fulltext, or semantic search. Returns ranked documents with highlights.",
    inputSchema: z.object({
      query: z.string().describe("The search query text (required)"),
      mode: z
        .enum(["hybrid", "fulltext", "semantic"])
        .default("hybrid")
        .describe(
          'Search mode: "hybrid" (default), "fulltext", or "semantic"'
        ),
      source_types: z
        .array(z.string())
        .optional()
        .describe(
          'Optional filter by source types, e.g. ["gmail","google_drive","slack","notion","hubspot"]'
        ),
      limit: z
        .number()
        .int()
        .min(1)
        .max(100)
        .default(20)
        .describe("Number of results to return (default: 20, max: 100)"),
      offset: z
        .number()
        .int()
        .min(0)
        .default(0)
        .describe("Pagination offset (default: 0)"),
    }),
    annotations: {
      readOnlyHint: true,
      destructiveHint: false,
    },
  },
  async ({ query, mode, source_types, limit, offset }) => {
    const payload: Record<string, unknown> = { query, mode, limit, offset };
    if (source_types && source_types.length > 0) {
      payload.source_types = source_types;
    }

    const data = (await omniPost("/api/v1/search", payload)) as SearchResponse;
    const text = formatSearchResults(data, query);
    return { content: [{ type: "text", text }] };
  }
);

// --- Tool: omni_document ---

server.registerTool(
  "omni_document",
  {
    title: "Omni Document",
    description:
      "Fetch the full content of a document from the Omni knowledge platform by its ID. Optionally retrieve a specific line range.",
    inputSchema: z.object({
      id: z.string().describe("The document ID to retrieve"),
      start_line: z
        .number()
        .int()
        .min(0)
        .optional()
        .describe("Optional: start line for partial content retrieval"),
      end_line: z
        .number()
        .int()
        .min(0)
        .optional()
        .describe("Optional: end line for partial content retrieval"),
    }),
    annotations: {
      readOnlyHint: true,
      destructiveHint: false,
    },
  },
  async ({ id, start_line, end_line }) => {
    const params = new URLSearchParams();
    if (start_line != null) params.set("start_line", String(start_line));
    if (end_line != null) params.set("end_line", String(end_line));

    const qs = params.toString();
    const path = `/api/v1/documents/${encodeURIComponent(id)}${qs ? `?${qs}` : ""}`;
    const doc = (await omniGet(path)) as DocumentResponse;
    const text = formatDocument(doc);
    return { content: [{ type: "text", text }] };
  }
);

// --- Tool: omni_status ---

server.registerTool(
  "omni_status",
  {
    title: "Omni Status",
    description:
      "Check the health and sync status of the Omni knowledge platform, including per-source connector status.",
    inputSchema: z.object({}),
    annotations: {
      readOnlyHint: true,
      destructiveHint: false,
    },
  },
  async () => {
    const health = (await omniGet("/api/v1/health", false)) as HealthResponse;

    let sources: SourcesResponse | null = null;
    try {
      sources = (await omniGet("/api/v1/sources")) as SourcesResponse;
    } catch (err) {
      console.error("Warning: could not fetch sources:", err);
    }

    const text = formatStatus(health, sources);
    return { content: [{ type: "text", text }] };
  }
);

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error(`Omni MCP server running (base URL: ${BASE_URL})`);
}

main().catch((err) => {
  console.error("Fatal error:", err);
  process.exit(1);
});
