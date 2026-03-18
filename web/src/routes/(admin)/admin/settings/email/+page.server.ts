import { fail } from '@sveltejs/kit'
import type { PageServerLoad, Actions } from './$types'
import { requireAdmin } from '$lib/server/authHelpers'
import {
    listActiveProviders,
    getProvider,
    createProvider,
    updateProvider,
    deleteProvider,
    setCurrentProvider,
    EMAIL_PROVIDER_TYPES,
    EMAIL_PROVIDER_LABELS,
    type EmailProviderConfig,
    type EmailProviderType,
} from '$lib/server/db/email-providers'
import { getEmailProvider, resetEmailProvider } from '$lib/server/email/factory'
import { ResendEmailProvider } from '$lib/server/email/providers/resend'
import { SMTPEmailProvider } from '$lib/server/email/providers/smtp'
import { ACSEmailProvider } from '$lib/server/email/providers/acs'

function stripSecrets(config: Record<string, unknown>): Record<string, unknown> {
    const { connectionString, apiKey, password, ...rest } = config
    return rest
}

export const load: PageServerLoad = async ({ locals }) => {
    requireAdmin(locals)

    const providers = await listActiveProviders()

    return {
        adminEmail: locals.user?.email ?? null,
        providers: providers.map((p) => {
            const config = p.config as Record<string, unknown>
            return {
                id: p.id,
                name: p.name,
                providerType: p.providerType,
                config: stripSecrets(config),
                hasSecret: !!(config.connectionString || config.apiKey || config.password),
                isCurrent: p.isCurrent,
            }
        }),
    }
}

export const actions: Actions = {
    add: async ({ request, locals }) => {
        requireAdmin(locals)

        const formData = await request.formData()
        const providerType = formData.get('providerType') as EmailProviderType

        if (!providerType || !EMAIL_PROVIDER_TYPES.includes(providerType))
            return fail(400, { error: 'Invalid provider type' })

        const result = buildConfig(formData, providerType)
        if ('error' in result) return fail(400, { error: result.error })

        const name = EMAIL_PROVIDER_LABELS[providerType] || providerType

        try {
            await createProvider({ name, providerType, config: result.config })
            resetEmailProvider()
            return { success: true, message: 'Email provider connected' }
        } catch (err) {
            console.error('Failed to add email provider:', err)
            return fail(500, { error: 'Failed to add provider' })
        }
    },

    edit: async ({ request, locals }) => {
        requireAdmin(locals)

        const formData = await request.formData()
        const id = formData.get('id') as string
        if (!id) return fail(400, { error: 'Provider ID is required' })

        const existing = await getProvider(id)
        if (!existing) return fail(404, { error: 'Provider not found' })

        const providerType = existing.providerType as EmailProviderType
        const existingConfig = {
            ...(existing.config as Record<string, unknown>),
            type: existing.providerType,
        } as EmailProviderConfig

        const result = buildConfig(formData, providerType, existingConfig)
        if ('error' in result) return fail(400, { error: result.error })

        try {
            await updateProvider(id, { config: result.config })
            resetEmailProvider()
            return { success: true, message: 'Provider updated' }
        } catch (err) {
            console.error('Failed to update email provider:', err)
            return fail(500, { error: 'Failed to update provider' })
        }
    },

    delete: async ({ request, locals }) => {
        requireAdmin(locals)

        const formData = await request.formData()
        const id = formData.get('id') as string
        if (!id) return fail(400, { error: 'Provider ID is required' })

        try {
            await deleteProvider(id)
            resetEmailProvider()
            return { success: true, message: 'Provider removed' }
        } catch (err) {
            console.error('Failed to delete email provider:', err)
            return fail(500, { error: 'Failed to delete provider' })
        }
    },

    setCurrent: async ({ request, locals }) => {
        requireAdmin(locals)

        const formData = await request.formData()
        const id = formData.get('id') as string
        if (!id) return fail(400, { error: 'Provider ID is required' })

        try {
            await setCurrentProvider(id)
            resetEmailProvider()
            return { success: true, message: 'Email provider switched' }
        } catch (err) {
            console.error('Failed to set current email provider:', err)
            return fail(500, { error: 'Failed to switch provider' })
        }
    },

    sendTest: async ({ locals }) => {
        requireAdmin(locals)

        const adminEmail = locals.user?.email
        if (!adminEmail) return fail(400, { error: 'Could not determine your email address' })

        resetEmailProvider()
        const provider = await getEmailProvider()
        if (!provider) return fail(400, { error: 'No email provider configured' })

        try {
            const result = await provider.sendTestEmail(adminEmail)
            if (result.success) {
                return { success: true, message: `Test email sent to ${adminEmail}` }
            }
            return fail(500, { error: result.error || 'Failed to send test email' })
        } catch (err) {
            console.error('Failed to send test email:', err)
            return fail(500, { error: 'Failed to send test email' })
        }
    },

    testConnection: async ({ request, locals }) => {
        requireAdmin(locals)

        const formData = await request.formData()
        const providerType = formData.get('providerType') as EmailProviderType
        const id = formData.get('id') as string

        try {
            let config: EmailProviderConfig
            if (id) {
                const dbProvider = await getProvider(id)
                if (!dbProvider) return fail(404, { error: 'Provider not found' })
                config = {
                    ...(dbProvider.config as Record<string, unknown>),
                    type: dbProvider.providerType,
                } as EmailProviderConfig
            } else {
                const result = buildConfig(formData, providerType)
                if ('error' in result) return fail(400, { error: result.error })
                config = result.config
            }

            const provider = instantiateProvider(config)
            if (!provider) return fail(400, { error: 'Could not create provider instance' })

            const connected = await provider.testConnection()
            if (connected) {
                return { success: true, message: 'Connection successful' }
            } else {
                return fail(400, { error: 'Connection failed. Please check your credentials.' })
            }
        } catch (err) {
            console.error('Email connection test failed:', err)
            return fail(500, { error: 'Connection test failed' })
        }
    },
}

function instantiateProvider(config: EmailProviderConfig) {
    switch (config.type) {
        case 'acs':
            return new ACSEmailProvider(config.connectionString, config.senderAddress)
        case 'resend':
            return new ResendEmailProvider(config.apiKey, config.fromEmail)
        case 'smtp':
            return new SMTPEmailProvider({
                host: config.host,
                port: config.port,
                user: config.user,
                password: config.password,
                secure: config.secure,
                fromEmail: config.fromEmail,
            })
    }
}

function buildConfig(
    formData: FormData,
    providerType: EmailProviderType,
    existing?: EmailProviderConfig,
): { config: EmailProviderConfig } | { error: string } {
    switch (providerType) {
        case 'acs': {
            const connectionString =
                (formData.get('connectionString') as string) ||
                (existing?.type === 'acs' ? existing.connectionString : null)
            const senderAddress = (formData.get('senderAddress') as string)?.trim() || null

            if (!connectionString) return { error: 'Connection string is required' }
            if (!senderAddress) return { error: 'Sender address is required' }

            return { config: { type: 'acs', connectionString, senderAddress } }
        }
        case 'resend': {
            const apiKey =
                (formData.get('apiKey') as string) ||
                (existing?.type === 'resend' ? existing.apiKey : null)
            const fromEmail = (formData.get('fromEmail') as string)?.trim() || null

            if (!apiKey) return { error: 'API key is required' }
            if (!fromEmail) return { error: 'From email is required' }

            return { config: { type: 'resend', apiKey, fromEmail } }
        }
        case 'smtp': {
            const host = (formData.get('host') as string)?.trim() || null
            const portStr = formData.get('port') as string
            const port = portStr ? parseInt(portStr, 10) : 587
            const user =
                (formData.get('user') as string) ||
                (existing?.type === 'smtp' ? existing.user : null)
            const password =
                (formData.get('password') as string) ||
                (existing?.type === 'smtp' ? existing.password : null)
            const secure = formData.get('secure') === 'true'
            const fromEmail = (formData.get('fromEmail') as string)?.trim() || null

            if (!host) return { error: 'SMTP host is required' }
            if (!user) return { error: 'SMTP username is required' }
            if (!password) return { error: 'SMTP password is required' }
            if (!fromEmail) return { error: 'From email is required' }

            return { config: { type: 'smtp', host, port, user, password, secure, fromEmail } }
        }
    }
}
