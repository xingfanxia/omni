import { env } from '$env/dynamic/private'
import { json } from '@sveltejs/kit'
import type { RequestHandler } from './$types'

/** Mirrors shared/src/models.rs::AttributeFilter (untagged enum). */
type AttributeFilter = string | string[] | { gte?: string; lte?: string }

interface AgentSearchRequest {
    query?: string
    source_types?: string[]
    content_types?: string[]
    attribute_filters?: Record<string, AttributeFilter>
    limit?: number
    offset?: number
    mode?: string
    include_facets?: boolean
}

interface SearcherQuery {
    query: string
    source_types?: string[]
    content_types?: string[]
    attribute_filters?: Record<string, AttributeFilter>
    limit: number
    offset: number
    mode: string
    include_facets?: boolean
    user_email?: string
    user_id?: string
}

export const POST: RequestHandler = async ({ request, fetch, locals }) => {
    if (!locals.user) {
        return json({ error: 'Unauthorized' }, { status: 401 })
    }

    const logger = locals.logger.child('v1-search')

    let body: AgentSearchRequest
    try {
        body = await request.json()
    } catch {
        return json({ error: 'Invalid JSON in request body' }, { status: 400 })
    }

    const query = typeof body.query === 'string' ? body.query.trim() : ''
    const attributeFilters = body.attribute_filters

    // Empty query is allowed ONLY when there's some other filter driving the
    // search — otherwise an empty `filter_only_search` against 150K docs is
    // wasteful. Valid "empty query" cases:
    //   1. Query string contains operators only ("last week in:telegram")
    //      — the operator parser will extract them, leaving "" as the final query
    //      BUT that parsing happens inside the searcher, so from our POV any
    //      non-empty original body.query is fine.
    //   2. attribute_filters narrows the search (e.g. one specific chat_title)
    //   3. source_types is narrowly specified by the caller
    // We accept case 1 by checking the RAW body.query (not the trimmed one),
    // and cases 2+3 via explicit filters.
    const rawQuery = typeof body.query === 'string' ? body.query : ''
    const hasNonEmptyInput = rawQuery.trim().length > 0
    const hasNarrowingFilter =
        !!attributeFilters ||
        (body.source_types && body.source_types.length > 0)
    if (!hasNonEmptyInput && !hasNarrowingFilter) {
        return json(
            {
                error:
                    'query is required (or provide source_types / attribute_filters for filtered browsing)',
            },
            { status: 400 },
        )
    }

    // Enforce API key source scoping
    const allowedSources = locals.apiKeyAllowedSources
    let sourceTypes = body.source_types

    if (allowedSources) {
        if (sourceTypes) {
            // Intersect: only allow sources that are both requested AND permitted
            sourceTypes = sourceTypes.filter((s: string) => allowedSources.includes(s))
            if (sourceTypes.length === 0) {
                return json({ results: [], total_count: 0, query_time_ms: 0, has_more: false, query })
            }
        } else {
            // No explicit filter — restrict to allowed sources
            sourceTypes = allowedSources
        }
    }

    const queryData: SearcherQuery = {
        query,
        source_types: sourceTypes,
        content_types: body.content_types,
        attribute_filters: attributeFilters,
        limit: typeof body.limit === 'number' ? Math.min(body.limit, 100) : 20,
        offset: typeof body.offset === 'number' ? body.offset : 0,
        mode: ['fulltext', 'semantic', 'hybrid'].includes(body.mode as string)
            ? (body.mode as string)
            : 'hybrid',
        include_facets: body.include_facets,
        // 'admin' scope: omit user_email → searcher skips permission filter → all docs
        // 'user'/'public' scope (or cookie auth): real user identity → user's permitted docs
        user_email:
            locals.apiKeyScope === 'admin' ? undefined : locals.user.email,
        user_id:
            locals.apiKeyScope === 'admin' ? undefined : locals.user.id,
    }

    logger.debug('Agent search request', { query: queryData.query, mode: queryData.mode })

    try {
        const response = await fetch(`${env.SEARCHER_URL}/search`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(queryData),
        })

        if (!response.ok) {
            logger.error('Searcher service error', undefined, {
                status: response.status,
                query: queryData.query,
            })
            return json(
                { error: 'Search service unavailable', status: response.status },
                { status: 502 },
            )
        }

        const results = await response.json()

        logger.info('Agent search completed', {
            query: queryData.query,
            results_count: results.results?.length ?? 0,
            total_count: results.total_count,
            query_time_ms: results.query_time_ms,
        })

        return json(results)
    } catch (error) {
        logger.error('Search request failed', error as Error, { query: queryData.query })
        return json({ error: 'Search request failed' }, { status: 500 })
    }
}
