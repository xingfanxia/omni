import type { EmailProvider } from './types'
import { ResendEmailProvider } from './providers/resend'
import { SMTPEmailProvider } from './providers/smtp'
import { ACSEmailProvider } from './providers/acs'
import { getCurrentProvider, type EmailProviderConfig } from '../db/email-providers'

let emailProvider: EmailProvider | null = null
let providerLoaded = false

export async function getEmailProvider(): Promise<EmailProvider | null> {
    if (providerLoaded) {
        return emailProvider
    }

    const dbProvider = await getCurrentProvider()

    if (!dbProvider) {
        providerLoaded = true
        return null
    }

    const config = dbProvider.config as EmailProviderConfig
    const type = dbProvider.providerType

    if (type === 'acs') {
        if (!config.connectionString || !config.senderAddress) {
            providerLoaded = true
            return null
        }
        emailProvider = new ACSEmailProvider(config.connectionString, config.senderAddress)
    } else if (type === 'resend') {
        if (!config.apiKey || !config.fromEmail) {
            providerLoaded = true
            return null
        }
        emailProvider = new ResendEmailProvider(config.apiKey, config.fromEmail)
    } else if (type === 'smtp') {
        if (!config.host || !config.user || !config.password || !config.fromEmail) {
            providerLoaded = true
            return null
        }
        emailProvider = new SMTPEmailProvider({
            host: config.host,
            port: config.port || undefined,
            user: config.user,
            password: config.password,
            secure: config.secure || undefined,
            fromEmail: config.fromEmail,
        })
    }

    providerLoaded = true
    return emailProvider
}

export function resetEmailProvider(): void {
    emailProvider = null
    providerLoaded = false
}
