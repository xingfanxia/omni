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
    const response = await fetch(
        `${config.services.aiServiceUrl}/agents/${params.agentId}/runs/${params.runId}/stream`,
        {
            headers: { 'x-user-id': locals.user.id },
        },
    )

    if (!response.ok || !response.body) {
        return json({ error: 'Failed to connect to stream' }, { status: 502 })
    }

    return new Response(response.body, {
        status: 200,
        headers: {
            'Content-Type': 'text/event-stream',
            'Cache-Control': 'no-cache',
            Connection: 'keep-alive',
        },
    })
}
