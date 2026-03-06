<script lang="ts">
    import type { GmailExtra } from '$lib/types/search'

    let { extra }: { extra: GmailExtra } = $props()

    let participants = $derived(
        (extra?.participants || []).filter((p) => !/no-?reply/i.test(p.split('@')[0])),
    )
    let displayParticipants = $derived(participants.slice(0, 3))
    let remaining = $derived(Math.max(0, participants.length - 3))
</script>

{#if participants.length > 0}
    <div class="mt-1 flex flex-wrap items-center gap-1.5">
        {#each displayParticipants as participant}
            <span
                class="bg-muted inline-flex rounded-full px-2 py-0.5 text-xs text-gray-700"
                title={participant}>
                {participant}
            </span>
        {/each}
        {#if remaining > 0}
            <span class="bg-muted inline-flex rounded-full px-2 py-0.5 text-xs text-gray-500">
                +{remaining}
            </span>
        {/if}
    </div>
{/if}
