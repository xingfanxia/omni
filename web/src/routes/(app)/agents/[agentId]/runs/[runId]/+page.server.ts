import type { PageServerLoad } from './$types.js'
import { requireActiveUser } from '$lib/server/authHelpers.js'
import { getAgent } from '$lib/server/db/agents.js'
import { getConfig } from '$lib/server/config.js'
import { error } from '@sveltejs/kit'

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

    const config = getConfig()
    const resp = await fetch(
        `${config.services.aiServiceUrl}/agents/${params.agentId}/runs/${params.runId}`,
        { headers: { 'x-user-id': user.id } },
    )

    if (!resp.ok) {
        throw error(resp.status, 'Run not found')
    }

    const run = await resp.json()
    return { user, agent, run }
}
