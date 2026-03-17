import { json } from '@sveltejs/kit'
import type { RequestHandler } from './$types.js'
import { getAgent } from '$lib/server/db/agents.js'
import { getConfig } from '$lib/server/config.js'

export const GET: RequestHandler = async ({ params, locals }) => {
    if (!locals.user?.id) {
        return json({ error: 'User not authenticated' }, { status: 401 })
    }

    const agent = await getAgent(params.agentId)
    if (!agent) {
        return json({ error: 'Agent not found' }, { status: 404 })
    }

    if (agent.agentType === 'org') {
        if (locals.user.role !== 'admin') {
            return json({ error: 'Admin access required' }, { status: 403 })
        }
    } else if (agent.userId !== locals.user.id) {
        return json({ error: 'Access denied' }, { status: 403 })
    }

    const config = getConfig()
    const response = await fetch(`${config.services.aiServiceUrl}/agents/${params.agentId}/runs`, {
        headers: { 'x-user-id': locals.user.id },
    })

    if (!response.ok) {
        return json({ error: 'Failed to fetch runs' }, { status: response.status })
    }

    const runs = await response.json()
    return json(runs)
}
