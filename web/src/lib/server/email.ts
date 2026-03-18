import { getEmailProvider } from './email/factory'
import type { EmailResult } from './email/types'

export { type EmailResult } from './email/types'

export class EmailService {
    static async sendMagicLink(
        email: string,
        magicLinkUrl: string,
        isNewUser: boolean = false,
    ): Promise<EmailResult> {
        try {
            const provider = await getEmailProvider()
            if (!provider) {
                return {
                    success: false,
                    error: 'No email provider configured. Please configure one in admin settings.',
                }
            }
            return await provider.sendMagicLink(email, magicLinkUrl, isNewUser)
        } catch (error) {
            console.error('Error sending magic link email:', error)
            return {
                success: false,
                error: 'Failed to send email',
            }
        }
    }

    static async testConnection(): Promise<boolean> {
        try {
            const provider = await getEmailProvider()
            if (!provider) return false
            return await provider.testConnection()
        } catch (error) {
            console.error('Email connection test failed:', error)
            return false
        }
    }
}
