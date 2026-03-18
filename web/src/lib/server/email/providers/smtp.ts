import { createTransport, type Transporter } from 'nodemailer'
import type { EmailProvider, EmailResult } from '../types'
import { generateMagicLinkHtml, generateMagicLinkText } from '../templates'

export class SMTPEmailProvider implements EmailProvider {
    private transporter: Transporter
    private fromEmail: string

    constructor(config: {
        host: string
        port?: number
        user: string
        password: string
        secure?: boolean
        fromEmail: string
    }) {
        this.fromEmail = config.fromEmail
        this.transporter = createTransport({
            host: config.host,
            port: config.port || 587,
            secure: config.secure || false,
            auth: {
                user: config.user,
                pass: config.password,
            },
        })
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
            const text = generateMagicLinkText(magicLinkUrl, email, isNewUser)

            const info = await this.transporter.sendMail({
                from: this.fromEmail,
                to: email,
                subject,
                html,
                text,
            })

            return {
                success: true,
                messageId: info.messageId,
            }
        } catch (error) {
            console.error('Error sending email via SMTP:', error)
            return {
                success: false,
                error: 'Failed to send email',
            }
        }
    }

    async testConnection(): Promise<boolean> {
        try {
            await this.transporter.verify()
            return true
        } catch (error) {
            console.error('SMTP connection test failed:', error)
            return false
        }
    }
}
