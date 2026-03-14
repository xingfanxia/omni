import { redirect } from '@sveltejs/kit'
import { getOktaAuthConfig } from '$lib/server/db/auth-providers'
import { app } from '$lib/server/config'
import { OAuthStateManager } from '$lib/server/oauth/state'
import { loadOktaOAuthService } from '$lib/server/oauth/okta'
import type { RequestHandler } from './$types'
import { logger } from '$lib/server/logger'

export const GET: RequestHandler = async ({ url }) => {
    let authUrl: string
    try {
        const config = await getOktaAuthConfig()
        if (
            !config ||
            !config.enabled ||
            !config.oktaDomain ||
            !config.clientId ||
            !config.clientSecret
        ) {
            logger.error('Okta SSO is not configured')
            throw redirect(302, '/login?error=oauth_not_configured')
        }

        const OktaOAuthService = await loadOktaOAuthService()
        if (!OktaOAuthService) {
            throw redirect(302, '/login?error=okta_not_available')
        }

        const redirectUri = url.searchParams.get('redirect_uri') || undefined

        if (redirectUri) {
            try {
                const redirectUrl = new URL(redirectUri)
                if (redirectUrl.origin !== url.origin) {
                    throw redirect(302, '/login?error=invalid_redirect')
                }
            } catch {
                throw redirect(302, '/login?error=invalid_redirect')
            }
        }

        const callbackUrl = `${app.publicUrl}/auth/okta/callback`
        const oktaService = new OktaOAuthService(
            {
                oktaDomain: config.oktaDomain,
                clientId: config.clientId,
                clientSecret: config.clientSecret,
            },
            callbackUrl,
        )

        const { url: oktaAuthUrl, state, codeVerifier } = await oktaService.createAuthorizationURL()

        // Store state + codeVerifier in Redis
        const { stateToken } = await OAuthStateManager.createState('okta', redirectUri, undefined, {
            codeVerifier,
            oktaState: state,
        })

        // Swap our state token into the auth URL
        const authUrlObj = new URL(oktaAuthUrl)
        authUrlObj.searchParams.set('state', stateToken)
        authUrl = authUrlObj.toString()
    } catch (error) {
        logger.error('Okta OAuth initiation error:', error)

        if (error instanceof Response) {
            throw error
        }

        throw redirect(302, '/login?error=oauth_error')
    }

    logger.info('Redirecting to Okta:', authUrl)
    throw redirect(302, authUrl)
}

export const POST: RequestHandler = GET
