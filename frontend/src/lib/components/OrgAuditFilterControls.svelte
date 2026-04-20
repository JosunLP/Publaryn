<script lang="ts">
  interface AuditActionOption {
    value: string;
    label: string;
  }

  interface AuditActorOption {
    userId: string;
    username: string;
    label: string;
  }

  export let actionOptions: readonly AuditActionOption[] = [];
  export let actionValue = '';
  export let actorInput = '';
  export let actorOptions: readonly AuditActorOption[] = [];
  export let occurredFrom = '';
  export let occurredUntil = '';
  export let exporting = false;
  export let summary = '';
  export let showActionClear = false;
  export let showActorClear = false;
  export let showDateClear = false;
  export let handleSubmit: (event: SubmitEvent) => void | Promise<void> = () => {};
  export let handleExport: () => void | Promise<void> = () => {};
  export let clearAction: () => void | Promise<void> = () => {};
  export let clearActor: () => void | Promise<void> = () => {};
  export let clearDates: () => void | Promise<void> = () => {};
</script>

<form on:submit={handleSubmit}>
  <div class="flex flex-wrap items-end gap-4">
    <div class="form-group" style="margin-bottom:0; min-width:240px;">
      <label for="org-audit-action">Action</label>
      <select
        id="org-audit-action"
        name="action"
        class="form-input"
        value={actionValue}
      >
        <option value="">All events</option>
        {#each actionOptions as action}
          <option value={action.value} selected={action.value === actionValue}
            >{action.label}</option
          >
        {/each}
      </select>
    </div>
    <div class="form-group" style="margin-bottom:0; min-width:260px;">
      <label for="org-audit-actor">Actor</label>
      <input
        id="org-audit-actor"
        name="actor_query"
        class="form-input"
        list="org-audit-actor-options"
        bind:value={actorInput}
        placeholder="Search username or paste user id"
        autocomplete="off"
      />
      <datalist id="org-audit-actor-options">
        {#each actorOptions as actor}
          <option value={actor.username}>{actor.label}</option>
          <option value={actor.userId}>{actor.label}</option>
        {/each}
      </datalist>
    </div>
    <div class="form-group" style="margin-bottom:0; min-width:180px;">
      <label for="org-audit-from">From (UTC)</label>
      <input
        id="org-audit-from"
        name="occurred_from"
        type="date"
        class="form-input"
        value={occurredFrom}
      />
    </div>
    <div class="form-group" style="margin-bottom:0; min-width:180px;">
      <label for="org-audit-until">Until (UTC)</label>
      <input
        id="org-audit-until"
        name="occurred_until"
        type="date"
        class="form-input"
        value={occurredUntil}
      />
    </div>
    <button type="submit" class="btn btn-secondary">Apply</button>
    <button
      type="button"
      class="btn btn-secondary"
      disabled={exporting}
      on:click={handleExport}
    >
      {exporting ? 'Exporting…' : 'Export CSV'}
    </button>
    {#if showActionClear}
      <button type="button" class="btn btn-secondary" on:click={clearAction}
        >Clear action</button
      >
    {/if}
    {#if showActorClear}
      <button type="button" class="btn btn-secondary" on:click={clearActor}
        >Clear actor</button
      >
    {/if}
    {#if showDateClear}
      <button type="button" class="btn btn-secondary" on:click={clearDates}
        >Clear dates</button
      >
    {/if}
  </div>
  <p class="settings-copy" style="margin-top:0.75rem; margin-bottom:0;">
    {summary}
  </p>
</form>
