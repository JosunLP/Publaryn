<script lang="ts">
  import type { SecurityFinding } from '../../api/packages';

  export let findings: readonly SecurityFinding[] = [];
  export let findingNotes: Record<string, string> = {};
  export let updatingFindingId: string | null = null;
  export let notePlaceholder = '';
  export let formatDateValue: (value: string | null | undefined) => string = (
    value
  ) => `${value || ''}`;
  export let normalizeSeverity: (value: string) => string = (value) => value;
  export let formatKindLabel: (value: string) => string = (value) => value;
  export let handleNoteInput: (
    findingId: string,
    value: string
  ) => void | Promise<void> = () => {};
  export let handleToggleResolution: (
    finding: SecurityFinding
  ) => void | Promise<void> = () => {};
</script>

<div class="token-list">
  {#each findings as finding}
    {@const severity = normalizeSeverity(finding.severity)}
    <div class="token-row">
      <div class="token-row__main">
        <div class="token-row__title">
          {finding.title}
        </div>
        <div class="token-row__meta">
          <span class={`badge badge-severity-${severity}`}>{severity}</span>
          <span class="badge badge-ecosystem"
            >{formatKindLabel(finding.kind)}</span
          >
          {#if finding.release_version}
            <span>{finding.release_version}</span>
          {/if}
          <span>{formatDateValue(finding.detected_at)}</span>
          {#if finding.is_resolved}
            <span
              >Resolved{finding.resolved_at
                ? ` ${formatDateValue(finding.resolved_at)}`
                : ''}</span
            >
          {/if}
        </div>
        {#if finding.description}
          <p class="settings-copy" style="margin-top:0.5rem; margin-bottom:0;">
            {finding.description}
          </p>
        {/if}
        <label
          class="form-group"
          style="margin-top:0.75rem; margin-bottom:0;"
        >
          <span class="sr-only">Security finding note for {finding.title}</span>
          <textarea
            class="form-input"
            rows="2"
            maxlength="2000"
            placeholder={notePlaceholder}
            value={findingNotes[finding.id] || ''}
            on:input={(event) =>
              handleNoteInput(
                finding.id,
                (event.currentTarget as HTMLTextAreaElement).value
              )}
          ></textarea>
        </label>
      </div>
      <div class="token-row__actions">
        <button
          type="button"
          class="btn btn-secondary btn-sm"
          disabled={updatingFindingId !== null}
          on:click={() => handleToggleResolution(finding)}
        >
          {#if updatingFindingId === finding.id}
            {finding.is_resolved ? 'Reopening…' : 'Resolving…'}
          {:else}
            {finding.is_resolved ? 'Reopen finding' : 'Mark resolved'}
          {/if}
        </button>
      </div>
    </div>
  {/each}
</div>
