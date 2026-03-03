<script lang="ts">
    import * as AlertDialog from '$lib/components/ui/alert-dialog'
    import { Button } from '$lib/components/ui/button'
    import { Loader2 } from '@lucide/svelte'
    import { goto } from '$app/navigation'
    import { toast } from 'svelte-sonner'

    let {
        open = $bindable(false),
        sourceId,
        sourceName,
    }: {
        open: boolean
        sourceId: string
        sourceName: string
    } = $props()

    let isRemoving = $state(false)

    async function handleRemove() {
        isRemoving = true
        try {
            const response = await fetch(`/api/sources/${sourceId}`, {
                method: 'DELETE',
            })

            if (!response.ok) {
                const data = await response.json().catch(() => null)
                throw new Error(data?.message || 'Failed to remove source')
            }

            toast.success(`${sourceName} has been removed`)
            open = false
            goto('/admin/settings/integrations')
        } catch (err) {
            toast.error(err instanceof Error ? err.message : 'Failed to remove source')
        } finally {
            isRemoving = false
        }
    }
</script>

<AlertDialog.Root bind:open>
    <AlertDialog.Content>
        <AlertDialog.Header>
            <AlertDialog.Title>Remove {sourceName}?</AlertDialog.Title>
            <AlertDialog.Description>
                This will permanently delete this connector and all its synced data, including
                documents, search index entries, and sync history. This action cannot be undone.
            </AlertDialog.Description>
        </AlertDialog.Header>
        <AlertDialog.Footer>
            <AlertDialog.Cancel disabled={isRemoving} class="cursor-pointer"
                >Cancel</AlertDialog.Cancel>
            <Button
                variant="destructive"
                class="cursor-pointer"
                onclick={handleRemove}
                disabled={isRemoving}>
                {#if isRemoving}
                    <Loader2 class="mr-2 h-4 w-4 animate-spin" />
                {/if}
                Remove
            </Button>
        </AlertDialog.Footer>
    </AlertDialog.Content>
</AlertDialog.Root>
