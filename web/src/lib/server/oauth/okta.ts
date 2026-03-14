import { logger } from '../logger'

export async function loadOktaOAuthService(): Promise<any | null> {
    try {
        // @ts-ignore — enterprise-only package, not present in base builds
        const mod = await import('@getomnico/okta-sso')
        return mod.OktaOAuthService
    } catch {
        logger.debug('Okta SSO enterprise package is not installed')
        return null
    }
}
