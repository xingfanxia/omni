import { json, error } from '@sveltejs/kit'
import type { RequestHandler } from './$types'
import { db } from '$lib/server/db'
import { sources, syncRuns, connectorEventsQueue } from '$lib/server/db/schema'
import { eq, and } from 'drizzle-orm'
import { getConfig } from '$lib/server/config'
import { logger } from '$lib/server/logger'

export const DELETE: RequestHandler = async ({ params, locals, fetch }) => {
    if (!locals.user) {
        throw error(401, 'Unauthorized')
    }

    if (locals.user.role !== 'admin') {
        throw error(403, 'Admin access required')
    }

    const sourceId = params.sourceId

    const source = await db.query.sources.findFirst({
        where: eq(sources.id, sourceId),
    })

    if (!source) {
        throw error(404, 'Source not found')
    }

    const config = getConfig()
    const connectorManagerUrl = config.services.connectorManagerUrl

    // Cancel any running sync for this source
    const runningSyncs = await db.query.syncRuns.findMany({
        where: and(eq(syncRuns.sourceId, sourceId), eq(syncRuns.status, 'running')),
    })

    for (const sync of runningSyncs) {
        try {
            await fetch(`${connectorManagerUrl}/sync/${sync.id}/cancel`, {
                method: 'POST',
            })
        } catch (err) {
            logger.warn(`Failed to cancel sync ${sync.id} for source ${sourceId}`, err)
        }
    }

    // Delete from connector_events_queue (no cascade FK on this table)
    await db.delete(connectorEventsQueue).where(eq(connectorEventsQueue.sourceId, sourceId))

    // Delete the source — cascades to documents, embeddings, service_credentials, sync_runs, webhook_channels
    await db.delete(sources).where(eq(sources.id, sourceId))

    return json({ success: true })
}
