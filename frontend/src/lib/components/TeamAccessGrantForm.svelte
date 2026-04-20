<script lang="ts">
  export interface TeamAccessGrantTargetOption {
    value: string;
    label: string;
  }

  export interface TeamAccessPermissionOption {
    value: string;
    label: string;
    description: string;
  }

  export let fieldId: string;
  export let selectLabel: string;
  export let selectName: string;
  export let placeholderLabel: string;
  export let emptyMessage: string;
  export let submitLabel: string;
  export let error: string | null = null;
  export let options: TeamAccessGrantTargetOption[] = [];
  export let permissionOptions: TeamAccessPermissionOption[] = [];
  export let handleSubmit: (event: SubmitEvent) => void | Promise<void> = () => {};

  $: isDisabled = Boolean(error) || options.length === 0;
</script>

<form class="settings-subsection" on:submit={handleSubmit}>
  <div class="form-group">
    <label for={fieldId}>{selectLabel}</label>
    {#if error}
      <div class="alert alert-error">{error}</div>
    {:else if options.length === 0}
      <p class="settings-copy">{emptyMessage}</p>
    {:else}
      <select id={fieldId} name={selectName} class="form-input" required>
        <option value="">{placeholderLabel}</option>
        {#each options as option}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
    {/if}
  </div>
  <fieldset class="form-group">
    <legend>Permissions</legend>
    <div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
      {#each permissionOptions as permission}
        <label class="rounded-lg border border-neutral-200 p-3 text-sm">
          <span class="flex items-start gap-3">
            <input
              type="checkbox"
              name="permissions"
              value={permission.value}
              disabled={isDisabled}
            />
            <span>
              <span class="block font-medium">{permission.label}</span>
              <span class="mt-1 block text-muted">{permission.description}</span>
            </span>
          </span>
        </label>
      {/each}
    </div>
  </fieldset>
  <button type="submit" class="btn btn-primary" disabled={isDisabled}
    >{submitLabel}</button
  >
</form>
