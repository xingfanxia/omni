import { Resend } from 'resend'
import type { EmailProvider, EmailResult } from '../types'
import { generateMagicLinkHtml } from '../templates'
import { createLogger } from '../../logger.js'

const logger = createLogger('resend-email')

export class ResendEmailProvider implements EmailProvider {
    private resend: Resend
    private fromEmail: string

    constructor(apiKey: string, fromEmail: string) {
        this.resend = new Resend(apiKey)
        this.fromEmail = fromEmail
    }

    async sendMagicLink(
        email: string,
        magicLinkUrl: string,
        isNewUser: boolean = false,
    ): Promise<EmailResult> {
        try {
            const subject = isNewUser
                ? 'Welcome to Omni - Complete your account setup'
                : 'Your Omni login link'

            const html = generateMagicLinkHtml(magicLinkUrl, email, isNewUser)

            const { data, error } = await this.resend.emails.send({
                from: this.fromEmail,
                to: [email],
                subject,
                html,
            })

            if (error) {
                logger.error('Resend error', error, { email })
                return {
                    success: false,
                    error: error.message || 'Failed to send email',
                }
            }

            return {
                success: true,
                messageId: data?.id,
            }
        } catch (error) {
            logger.error('Error sending email via Resend', error, { email })
            return {
                success: false,
                error: 'Failed to send email',
            }
        }
    }

    async testConnection(): Promise<boolean> {
        try {
            const { error } = await this.resend.domains.list()
            return !error
        } catch (error) {
            logger.error('Resend connection test failed', error)
            return false
        }
    }
}
