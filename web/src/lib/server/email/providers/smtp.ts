import { createTransport, type Transporter } from 'nodemailer'
import { EmailProvider, type EmailResult, type SendEmailParams } from '../types'

export class SMTPEmailProvider extends EmailProvider {
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
        super()
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

    async send(params: SendEmailParams): Promise<EmailResult> {
        try {
            const info = await this.transporter.sendMail({
                from: this.fromEmail,
                to: params.to,
                subject: params.subject,
                html: params.html,
                text: params.text,
            })

            return { success: true, messageId: info.messageId }
        } catch (error) {
            console.error('Error sending email via SMTP:', error)
            return { success: false, error: 'Failed to send email' }
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
