import type { PageServerLoad } from './$types.js'
import { requireActiveUser } from '$lib/server/authHelpers.js'
import { getConfig } from '$lib/server/config.js'
import { listAllActiveModels } from '$lib/server/db/model-providers.js'

export const load: PageServerLoad = async ({ locals }) => {
    const { user } = requireActiveUser(locals)

    // Fetch user's connected sources
    let sources: any[] = []
    try {
        const config = getConfig()
        const resp = await fetch(`${config.services.connectorManagerUrl}/sources`)
        if (resp.ok) {
            const allSources = await resp.json()
            sources = allSources.filter((s: any) => s.is_active && !s.is_deleted)
        }
    } catch {
        // Sources unavailable
    }

    const models = await listAllActiveModels()

    return { user, sources, models }
}
