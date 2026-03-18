import { EmailClient } from '@azure/communication-email'
import type { EmailProvider, EmailResult } from '../types'
import { generateMagicLinkHtml, generateMagicLinkText } from '../templates'
import { createLogger } from '../../logger.js'

const logger = createLogger('acs-email')

export class ACSEmailProvider implements EmailProvider {
    private client: EmailClient
    private senderAddress: string

    constructor(connectionString: string, senderAddress: string) {
        this.client = new EmailClient(connectionString)
        this.senderAddress = senderAddress
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
            const plainText = generateMagicLinkText(magicLinkUrl, email, isNewUser)

            const message = {
                senderAddress: this.senderAddress,
                content: {
                    subject,
                    html,
                    plainText,
                },
                recipients: {
                    to: [{ address: email }],
                },
            }

            const poller = await this.client.beginSend(message)
            const result = await poller.pollUntilDone()

            if (result.status === 'Succeeded') {
                return {
                    success: true,
                    messageId: result.id,
                }
            }

            logger.error('ACS send failed', { status: result.status, email })
            return {
                success: false,
                error: `Email send failed with status: ${result.status}`,
            }
        } catch (error) {
            logger.error('Error sending email via ACS', error, { email })
            return {
                success: false,
                error: 'Failed to send email',
            }
        }
    }

    async testConnection(): Promise<boolean> {
        try {
            // Validate the connection string by attempting to create a send operation
            // with an invalid recipient. ACS will authenticate before validating the message,
            // so an auth error means bad credentials, while other errors mean it's connected.
            const message = {
                senderAddress: this.senderAddress,
                content: {
                    subject: 'Connection Test',
                    plainText: 'Test',
                },
                recipients: {
                    to: [{ address: 'test@test.invalid' }],
                },
            }

            const poller = await this.client.beginSend(message)
            await poller.pollUntilDone()
            return true
        } catch (error: any) {
            if (error?.code === 'Unauthorized' || error?.statusCode === 401) {
                return false
            }
            // Other errors (like invalid recipient) still indicate the connection works
            return true
        }
    }
}
