<script lang="ts">
    import * as Dialog from '$lib/components/ui/dialog'
    import { Button } from '$lib/components/ui/button'
    import { Input } from '$lib/components/ui/input'
    import { Label } from '$lib/components/ui/label'
    import { AuthType } from '$lib/types'
    import { toast } from 'svelte-sonner'

    interface Props {
        open: boolean
        onSuccess?: () => void
        onCancel?: () => void
    }

    let { open = $bindable(false), onSuccess, onCancel }: Props = $props()

    let mode = $state<'telethon' | 'bot'>('telethon')
    let apiId = $state('')
    let apiHash = $state('')
    let sessionString = $state('')
    let botToken = $state('')
    let isSubmitting = $state(false)

    async function handleSubmit() {
        isSubmitting = true
        try {
            if (mode === 'telethon') {
                if (!apiId.trim() || !apiHash.trim() || !sessionString.trim()) {
                    throw new Error('API ID, API Hash, and Session String are all required')
                }
                if (!/^\d+$/.test(apiId.trim())) {
                    throw new Error('API ID must be a number')
                }
            } else {
                if (!botToken.trim()) {
                    throw new Error('Bot token is required')
                }
            }

            const sourceResponse = await fetch('/api/sources', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    name: 'Telegram',
                    sourceType: 'telegram',
                    config: {},
                }),
            })

            if (!sourceResponse.ok) {
                const err = await sourceResponse.json().catch(() => null)
                throw new Error(err?.message || 'Failed to create Telegram source')
            }

            const source = await sourceResponse.json()

            const credentials =
                mode === 'telethon'
                    ? {
                          api_id: apiId.trim(),
                          api_hash: apiHash.trim(),
                          session: sessionString.trim(),
                      }
                    : { token: botToken.trim() }

            const credentialsResponse = await fetch('/api/service-credentials', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    sourceId: source.id,
                    provider: 'telegram',
                    authType: mode === 'telethon' ? AuthType.API_KEY : AuthType.BOT_TOKEN,
                    credentials,
                }),
            })

            if (!credentialsResponse.ok) {
                throw new Error('Failed to store Telegram credentials')
            }

            toast.success('Telegram connected! Configure chat selection in settings.')
            open = false
            resetForm()
            onSuccess?.()
        } catch (error: any) {
            console.error('Error setting up Telegram:', error)
            toast.error(error.message || 'Failed to set up Telegram')
        } finally {
            isSubmitting = false
        }
    }

    function handleCancel() {
        open = false
        resetForm()
        onCancel?.()
    }

    function resetForm() {
        apiId = ''
        apiHash = ''
        sessionString = ''
        botToken = ''
        mode = 'telethon'
    }
</script>

<Dialog.Root bind:open>
    <Dialog.Content class="max-w-2xl">
        <Dialog.Header>
            <Dialog.Title>Connect Telegram</Dialog.Title>
            <Dialog.Description>
                Index messages from Telegram chats, groups, and channels.
            </Dialog.Description>
        </Dialog.Header>

        <div class="space-y-4">
            <!-- Mode selector -->
            <div class="space-y-2">
                <Label>Connection Mode</Label>
                <div class="flex gap-4">
                    <label class="flex cursor-pointer items-center gap-2">
                        <input
                            type="radio"
                            name="mode"
                            value="telethon"
                            bind:group={mode}
                            class="accent-primary" />
                        <span class="text-sm font-medium">User Session (full history)</span>
                    </label>
                    <label class="flex cursor-pointer items-center gap-2">
                        <input
                            type="radio"
                            name="mode"
                            value="bot"
                            bind:group={mode}
                            class="accent-primary" />
                        <span class="text-sm font-medium">Bot Token (forward-only)</span>
                    </label>
                </div>
            </div>

            {#if mode === 'telethon'}
                <div class="space-y-2">
                    <Label for="api-id">API ID</Label>
                    <Input
                        id="api-id"
                        bind:value={apiId}
                        placeholder="12345678"
                        required />
                    <p class="text-muted-foreground text-sm">
                        Get your API ID from <a
                            href="https://my.telegram.org/apps"
                            target="_blank"
                            class="text-blue-600 hover:underline">my.telegram.org/apps</a>
                    </p>
                </div>

                <div class="space-y-2">
                    <Label for="api-hash">API Hash</Label>
                    <Input
                        id="api-hash"
                        bind:value={apiHash}
                        placeholder="a1b2c3d4e5f6..."
                        type="password"
                        required />
                </div>

                <div class="space-y-2">
                    <Label for="session-string">Session String</Label>
                    <Input
                        id="session-string"
                        bind:value={sessionString}
                        placeholder="1BVtsO..."
                        type="password"
                        required />
                    <p class="text-muted-foreground text-sm">
                        Generate a session string by running the auth script:
                        <code class="bg-muted rounded px-1 text-xs"
                            >python connectors/telegram/scripts/auth.py --api_id YOUR_ID --api_hash YOUR_HASH</code>
                    </p>
                </div>
            {:else}
                <div class="space-y-2">
                    <Label for="bot-token">Bot Token</Label>
                    <Input
                        id="bot-token"
                        bind:value={botToken}
                        placeholder="123456:ABC-DEF..."
                        type="password"
                        required />
                    <p class="text-muted-foreground text-sm">
                        Create a bot via <a
                            href="https://t.me/BotFather"
                            target="_blank"
                            class="text-blue-600 hover:underline">@BotFather</a>
                        and copy the token. Note: Bot API only indexes messages received after
                        setup.
                    </p>
                </div>
            {/if}
        </div>

        <Dialog.Footer>
            <Button variant="outline" onclick={handleCancel} class="cursor-pointer">Cancel</Button>
            <Button onclick={handleSubmit} disabled={isSubmitting} class="cursor-pointer">
                {isSubmitting ? 'Connecting...' : 'Connect'}
            </Button>
        </Dialog.Footer>
    </Dialog.Content>
</Dialog.Root>
