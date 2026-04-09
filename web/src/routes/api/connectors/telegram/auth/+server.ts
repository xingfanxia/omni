import { json, error } from '@sveltejs/kit'
import type { RequestHandler } from './$types'
import { getConfig } from '$lib/server/config'
import { logger } from '$lib/server/logger'

// Pre-source auth action dispatch for the Telegram connector.
//
// Unlike `/api/sources/:id/action` (which looks up credentials from an
// existing source), this route runs auth actions BEFORE a source exists —
// the whole point is to help the user build their initial credentials via
// the web UI instead of `scripts/auth.py`.
//
// Strategy: ask connector-manager which URL the telegram connector is
// listening on, then POST directly to its `/action` endpoint with an empty
// credentials envelope. The Python connector's `auth_send_code` and
// `auth_verify_code` actions take all their input from `params`, so they
// don't need a real credentials lookup.
//
// Body: { action: "auth_send_code" | "auth_verify_code", params: {...} }

interface ConnectorInfo {
    source_type: string
    url: string
    healthy: boolean
}

const ALLOWED_ACTIONS = new Set(['auth_send_code', 'auth_verify_code'])

export const POST: RequestHandler = async ({ locals, request }) => {
    if (!locals.user) {
        throw error(401, 'Unauthorized')
    }
    if (locals.user.role !== 'admin') {
        throw error(403, 'Admin access required')
    }

    let body: { action?: unknown; params?: unknown }
    try {
        body = await request.json()
    } catch {
        throw error(400, 'Invalid JSON body')
    }

    const action = typeof body.action === 'string' ? body.action : ''
    const params =
        body.params && typeof body.params === 'object'
            ? (body.params as Record<string, unknown>)
            : {}

    if (!ALLOWED_ACTIONS.has(action)) {
        throw error(
            400,
            `Unsupported action: ${action}. Expected one of: ${[...ALLOWED_ACTIONS].join(', ')}`,
        )
    }

    const config = getConfig()
    const connectorManagerUrl = config.services.connectorManagerUrl

    // Look up the telegram connector's URL from the connector registry.
    let telegramConnectorUrl: string | undefined
    try {
        const res = await fetch(`${connectorManagerUrl}/connectors`)
        if (!res.ok) {
            throw new Error(`connector-manager /connectors returned ${res.status}`)
        }
        const connectors = (await res.json()) as ConnectorInfo[]
        const match = connectors.find((c) => c.source_type === 'telegram' && c.url)
        telegramConnectorUrl = match?.url
    } catch (err) {
        logger.error('Failed to discover telegram connector URL', { err })
        throw error(502, 'Unable to reach connector-manager')
    }

    if (!telegramConnectorUrl) {
        throw error(
            503,
            'Telegram connector is not registered. Is the telegram-connector service running?',
        )
    }

    // Call the Python connector directly. No credentials needed — the auth
    // actions derive everything from `params`.
    let actionRes: Response
    try {
        actionRes = await fetch(`${telegramConnectorUrl}/action`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                action,
                params,
                credentials: {},
            }),
        })
    } catch (err) {
        logger.error('Telegram connector unreachable', { err, telegramConnectorUrl })
        throw error(502, 'Telegram connector unreachable')
    }

    if (!actionRes.ok) {
        const errBody = await actionRes.text().catch(() => '(no body)')
        logger.error('Telegram connector returned non-200', {
            status: actionRes.status,
            body: errBody,
        })
        throw error(actionRes.status, errBody || 'Connector error')
    }

    const result = (await actionRes.json()) as {
        status?: string
        result?: unknown
        error?: string | null
    }

    if (result.error) {
        return json({ ok: false, error: result.error }, { status: 400 })
    }
    return json({ ok: true, result: result.result ?? {} })
}
