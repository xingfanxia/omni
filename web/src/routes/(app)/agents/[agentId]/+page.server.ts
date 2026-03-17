import type { PageServerLoad } from './$types.js'
import { requireActiveUser } from '$lib/server/authHelpers.js'
import { getAgent } from '$lib/server/db/agents.js'
import { getConfig } from '$lib/server/config.js'
import { error } from '@sveltejs/kit'
import { listAllActiveModels } from '$lib/server/db/model-providers.js'

export const load: PageServerLoad = async ({ locals, params }) => {
    const { user } = requireActiveUser(locals)

    const agent = await getAgent(params.agentId)
    if (!agent) {
        throw error(404, 'Agent not found')
    }
    if (agent.agentType !== 'org' && agent.userId !== user.id) {
        throw error(403, 'Access denied')
    }
    if (agent.agentType === 'org' && user.role !== 'admin') {
        throw error(403, 'Admin access required')
    }

    // Fetch runs from omni-ai
    let runs: any[] = []
    try {
        const config = getConfig()
        const resp = await fetch(`${config.services.aiServiceUrl}/agents/${params.agentId}/runs`, {
            headers: { 'x-user-id': user.id },
        })
        if (resp.ok) {
            runs = await resp.json()
        }
    } catch {
        // Runs unavailable
    }

    const models = await listAllActiveModels()

    return { user, agent, runs, models }
}
