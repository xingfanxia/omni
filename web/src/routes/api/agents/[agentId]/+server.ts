import { json } from '@sveltejs/kit'
import type { RequestHandler } from './$types.js'
import { getAgent, updateAgent, deleteAgent } from '$lib/server/db/agents.js'

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

    return json(agent)
}

export const PUT: RequestHandler = async ({ params, request, locals }) => {
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

    const data = await request.json()
    const updated = await updateAgent(params.agentId, data)
    return json(updated)
}

export const DELETE: RequestHandler = async ({ params, locals }) => {
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

    await deleteAgent(params.agentId)
    return json({ success: true })
}
