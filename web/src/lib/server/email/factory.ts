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

    const config = {
        ...(dbProvider.config as Record<string, unknown>),
        type: dbProvider.providerType,
    } as EmailProviderConfig

    switch (config.type) {
        case 'acs':
            emailProvider = new ACSEmailProvider(config.connectionString, config.senderAddress)
            break
        case 'resend':
            emailProvider = new ResendEmailProvider(config.apiKey, config.fromEmail)
            break
        case 'smtp':
            emailProvider = new SMTPEmailProvider({
                host: config.host,
                port: config.port,
                user: config.user,
                password: config.password,
                secure: config.secure,
                fromEmail: config.fromEmail,
            })
            break
    }

    providerLoaded = true
    return emailProvider
}

export function resetEmailProvider(): void {
    emailProvider = null
    providerLoaded = false
}
