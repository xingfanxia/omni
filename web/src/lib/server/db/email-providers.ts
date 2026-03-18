import { eq, and } from 'drizzle-orm'
import { db } from './index'
import { emailProviders } from './schema'
import type { EmailProvider } from './schema'
import { ulid } from 'ulid'

export { EMAIL_PROVIDER_TYPES, EMAIL_PROVIDER_LABELS, type EmailProviderType } from '$lib/types'

export interface ACSEmailProviderConfig {
    type: 'acs'
    connectionString: string
    senderAddress: string
}

export interface ResendEmailProviderConfig {
    type: 'resend'
    apiKey: string
    fromEmail: string
}

export interface SMTPEmailProviderConfig {
    type: 'smtp'
    host: string
    port: number
    user: string
    password: string
    secure: boolean
    fromEmail: string
}

export type EmailProviderConfig =
    | ACSEmailProviderConfig
    | ResendEmailProviderConfig
    | SMTPEmailProviderConfig

export interface CreateEmailProviderInput {
    name: string
    providerType: string
    config: EmailProviderConfig
}

export interface UpdateEmailProviderInput {
    name?: string
    config?: EmailProviderConfig
}

export async function listActiveProviders(): Promise<EmailProvider[]> {
    return await db
        .select()
        .from(emailProviders)
        .where(eq(emailProviders.isDeleted, false))
        .orderBy(emailProviders.createdAt)
}

export async function getProvider(id: string): Promise<EmailProvider | null> {
    const [provider] = await db
        .select()
        .from(emailProviders)
        .where(eq(emailProviders.id, id))
        .limit(1)
    return provider || null
}

export async function getCurrentProvider(): Promise<EmailProvider | null> {
    const [provider] = await db
        .select()
        .from(emailProviders)
        .where(and(eq(emailProviders.isCurrent, true), eq(emailProviders.isDeleted, false)))
        .limit(1)
    return provider || null
}

export async function createProvider(input: CreateEmailProviderInput): Promise<EmailProvider> {
    const existing = await getCurrentProvider()
    const shouldBeCurrent = !existing

    const [provider] = await db
        .insert(emailProviders)
        .values({
            id: ulid(),
            name: input.name,
            providerType: input.providerType,
            config: input.config,
            isCurrent: shouldBeCurrent,
        })
        .returning()

    return provider
}

export async function updateProvider(
    id: string,
    input: UpdateEmailProviderInput,
): Promise<EmailProvider | null> {
    const values: Record<string, unknown> = { updatedAt: new Date() }
    if (input.name !== undefined) values.name = input.name
    if (input.config !== undefined) values.config = input.config

    const [updated] = await db
        .update(emailProviders)
        .set(values)
        .where(eq(emailProviders.id, id))
        .returning()

    return updated || null
}

export async function deleteProvider(id: string): Promise<boolean> {
    const [updated] = await db
        .update(emailProviders)
        .set({ isDeleted: true, isCurrent: false, updatedAt: new Date() })
        .where(eq(emailProviders.id, id))
        .returning()

    return !!updated
}

export async function setCurrentProvider(id: string): Promise<{ previous: EmailProvider | null }> {
    const previous = await getCurrentProvider()

    await db
        .update(emailProviders)
        .set({ isCurrent: false, updatedAt: new Date() })
        .where(eq(emailProviders.isCurrent, true))

    await db
        .update(emailProviders)
        .set({ isCurrent: true, updatedAt: new Date() })
        .where(and(eq(emailProviders.id, id), eq(emailProviders.isDeleted, false)))

    return { previous }
}

export { type EmailProvider }
