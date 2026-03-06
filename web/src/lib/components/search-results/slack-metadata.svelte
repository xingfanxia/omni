<script lang="ts">
    import type { SlackExtra, DocumentMetadata } from '$lib/types/search'

    let { extra, metadata }: { extra: SlackExtra; metadata?: DocumentMetadata } = $props()

    let slack = $derived(extra?.slack)
    let channelName = $derived(metadata?.path || '')
    let authors = $derived(slack?.authors || [])
    let displayAuthors = $derived(authors.slice(0, 3))
    let remainingAuthors = $derived(Math.max(0, authors.length - 3))
    let messageCount = $derived(slack?.message_count)
</script>

{#if channelName || authors.length > 0 || messageCount}
    <div class="flex flex-wrap items-center gap-2 text-xs text-gray-500">
        {#if channelName}
            <span class="bg-muted text-muted-foreground rounded-full px-2 py-0.5 font-medium"
                >{channelName}</span>
        {/if}
        {#each displayAuthors as author}
            <span class="bg-muted text-muted-foreground rounded-full px-2 py-0.5">
                {author}
            </span>
        {/each}
        {#if remainingAuthors > 0}
            <span class="bg-muted text-muted-foreground rounded-full px-2 py-0.5">
                +{remainingAuthors}
            </span>
        {/if}
        {#if messageCount}
            <span class="text-gray-400">({messageCount} messages)</span>
        {/if}
    </div>
{/if}
