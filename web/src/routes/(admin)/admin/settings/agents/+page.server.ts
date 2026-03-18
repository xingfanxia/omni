import type { PageServerLoad } from './$types.js'
import { requireAdmin, requireActiveUser } from '$lib/server/authHelpers.js'
import { listOrgAgents } from '$lib/server/db/agents.js'

export const load: PageServerLoad = async ({ locals }) => {
    requireActiveUser(locals)
    const { user } = requireAdmin(locals)
    const agents = await listOrgAgents()
    return { user, agents }
}
