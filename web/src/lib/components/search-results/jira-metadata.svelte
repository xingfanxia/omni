<script lang="ts">
    import type { JiraAttributes } from '$lib/types/search'
    import { User } from '@lucide/svelte'

    let { attributes }: { attributes?: JiraAttributes } = $props()

    let issueKey = $derived(attributes?.issue_key)
    let issueType = $derived(attributes?.issue_type)
    let status = $derived(attributes?.status)
    let statusCategory = $derived(attributes?.status_category)
    let priority = $derived(attributes?.priority)
    let assignee = $derived(attributes?.assignee)

    function getStatusStyle(category?: string): string {
        switch (category) {
            case 'done':
                return 'status-done'
            case 'indeterminate':
                return 'status-progress'
            case 'new':
                return 'status-new'
            default:
                return 'status-new'
        }
    }

    function getIssueTypeStyle(type?: string): string {
        switch (type?.toLowerCase()) {
            case 'bug':
                return 'type-bug'
            case 'story':
            case 'user story':
                return 'type-story'
            case 'epic':
                return 'type-epic'
            case 'task':
            case 'sub-task':
                return 'type-task'
            default:
                return 'type-task'
        }
    }

    function getPriorityStyle(p?: string): string {
        switch (p?.toLowerCase()) {
            case 'highest':
            case 'critical':
                return 'priority-critical'
            case 'high':
                return 'priority-high'
            case 'medium':
                return 'priority-medium'
            case 'low':
            case 'lowest':
                return 'priority-low'
            default:
                return 'priority-medium'
        }
    }
</script>

{#if issueKey || status || assignee || priority}
    <div class="mt-1 flex flex-wrap items-center gap-1.5">
        {#if issueKey}
            <span class="bg-muted text-muted-foreground pill font-mono font-medium">
                {issueKey}
            </span>
        {/if}
        {#if issueType}
            <span class="pill {getIssueTypeStyle(issueType)}">
                {issueType}
            </span>
        {/if}
        {#if status}
            <span class="pill font-medium {getStatusStyle(statusCategory)}">
                {status}
            </span>
        {/if}
        {#if priority}
            <span class="pill {getPriorityStyle(priority)}">
                {priority}
            </span>
        {/if}
        {#if assignee}
            <span class="bg-muted text-muted-foreground pill inline-flex items-center gap-1">
                <User class="h-3 w-3 opacity-50" />
                {assignee}
            </span>
        {/if}
    </div>
{/if}

<style>
    .pill {
        display: inline-flex;
        align-items: center;
        border-radius: 9999px;
        padding: 0.125rem 0.5rem;
        font-size: 0.75rem;
        line-height: 1rem;
    }

    /* Issue type */
    .type-bug {
        background: oklch(0.95 0.05 25);
        color: oklch(0.45 0.15 25);
    }
    .type-story {
        background: oklch(0.95 0.04 145);
        color: oklch(0.4 0.12 145);
    }
    .type-epic {
        background: oklch(0.94 0.05 290);
        color: oklch(0.45 0.15 290);
    }
    .type-task {
        background: oklch(0.95 0.03 250);
        color: oklch(0.45 0.1 250);
    }

    /* Status */
    .status-done {
        background: oklch(0.95 0.04 155);
        color: oklch(0.4 0.12 155);
    }
    .status-progress {
        background: oklch(0.94 0.04 250);
        color: oklch(0.42 0.12 250);
    }
    .status-new {
        background: oklch(0.95 0.01 250);
        color: oklch(0.5 0.03 250);
    }

    /* Priority */
    .priority-critical {
        background: oklch(0.95 0.06 25);
        color: oklch(0.45 0.18 25);
    }
    .priority-high {
        background: oklch(0.95 0.05 55);
        color: oklch(0.48 0.14 55);
    }
    .priority-medium {
        background: oklch(0.95 0.04 85);
        color: oklch(0.48 0.12 85);
    }
    .priority-low {
        background: oklch(0.95 0.03 250);
        color: oklch(0.5 0.08 250);
    }

    :global(.dark) .type-bug {
        background: oklch(0.28 0.06 25);
        color: oklch(0.78 0.1 25);
    }
    :global(.dark) .type-story {
        background: oklch(0.28 0.05 145);
        color: oklch(0.78 0.08 145);
    }
    :global(.dark) .type-epic {
        background: oklch(0.28 0.06 290);
        color: oklch(0.78 0.1 290);
    }
    :global(.dark) .type-task {
        background: oklch(0.28 0.04 250);
        color: oklch(0.78 0.07 250);
    }

    :global(.dark) .status-done {
        background: oklch(0.28 0.05 155);
        color: oklch(0.78 0.08 155);
    }
    :global(.dark) .status-progress {
        background: oklch(0.28 0.05 250);
        color: oklch(0.78 0.08 250);
    }
    :global(.dark) .status-new {
        background: oklch(0.28 0.02 250);
        color: oklch(0.7 0.03 250);
    }

    :global(.dark) .priority-critical {
        background: oklch(0.28 0.07 25);
        color: oklch(0.78 0.12 25);
    }
    :global(.dark) .priority-high {
        background: oklch(0.28 0.06 55);
        color: oklch(0.78 0.1 55);
    }
    :global(.dark) .priority-medium {
        background: oklch(0.28 0.05 85);
        color: oklch(0.78 0.08 85);
    }
    :global(.dark) .priority-low {
        background: oklch(0.28 0.04 250);
        color: oklch(0.78 0.06 250);
    }
</style>
