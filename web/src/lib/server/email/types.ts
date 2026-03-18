export interface EmailResult {
    success: boolean
    messageId?: string
    error?: string
}

export interface EmailProvider {
    sendMagicLink(email: string, magicLinkUrl: string, isNewUser?: boolean): Promise<EmailResult>

    testConnection(): Promise<boolean>
}
