import { logger } from '../logger'

export async function loadEntraOAuthService(): Promise<any | null> {
    try {
        // @ts-ignore — enterprise-only package, not present in base builds
        const mod = await import('@getomnico/entra-sso')
        return mod.EntraOAuthService
    } catch {
        logger.debug('Entra SSO enterprise package is not installed')
        return null
    }
}
