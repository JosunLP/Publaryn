<script lang="ts">
  interface SeverityOption {
    value: string;
    label: string;
  }

  interface EcosystemOption {
    value: string;
    label: string;
  }

  interface PackageOption {
    value: string;
    label: string;
  }

  export let formClass = '';
  export let severityOptions: readonly SeverityOption[] = [];
  export let selectedSeverities: readonly string[] = [];
  export let ecosystemOptions: readonly EcosystemOption[] = [];
  export let ecosystemValue = '';
  export let packageValue = '';
  export let packageOptions: readonly PackageOption[] = [];
  export let exporting = false;
  export let summary = '';
  export let showSeverityClear = false;
  export let showEcosystemClear = false;
  export let showPackageClear = false;
  export let handleSubmit: (event: SubmitEvent) => void | Promise<void> = () => {};
  export let handleExport: () => void | Promise<void> = () => {};
  export let clearSeverity: () => void | Promise<void> = () => {};
  export let clearEcosystem: () => void | Promise<void> = () => {};
  export let clearPackage: () => void | Promise<void> = () => {};
</script>

<form class={formClass} on:submit={handleSubmit}>
  <div class="flex flex-wrap items-end gap-4">
    <fieldset class="form-group" style="margin-bottom:0; min-width:320px;">
      <legend>Severity</legend>
      <div class="token-row__scopes">
        {#each severityOptions as severity}
          <label class="badge badge-ecosystem">
            <input
              type="checkbox"
              name="security_severity"
              value={severity.value}
              checked={selectedSeverities.includes(severity.value)}
              style="margin-right:0.35rem;"
            />
            {severity.label}
          </label>
        {/each}
      </div>
    </fieldset>
    <div class="form-group" style="margin-bottom:0; min-width:220px;">
      <label for="org-security-ecosystem">Ecosystem</label>
      <select
        id="org-security-ecosystem"
        name="security_ecosystem"
        class="form-input"
        value={ecosystemValue}
      >
        <option value="">All ecosystems</option>
        {#each ecosystemOptions as ecosystem}
          <option
            value={ecosystem.value}
            selected={ecosystem.value === ecosystemValue}>{ecosystem.label}</option
          >
        {/each}
      </select>
    </div>
    <div class="form-group" style="margin-bottom:0; min-width:260px;">
      <label for="org-security-package">Package name</label>
      <input
        id="org-security-package"
        name="security_package"
        class="form-input"
        list="org-security-package-options"
        value={packageValue}
        placeholder="Match package name"
        autocomplete="off"
      />
      <datalist id="org-security-package-options">
        {#each packageOptions as pkg}
          <option value={pkg.value}>{pkg.label}</option>
        {/each}
      </datalist>
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
    {#if showSeverityClear}
      <button
        type="button"
        class="btn btn-secondary"
        on:click={clearSeverity}>Clear severity</button
      >
    {/if}
    {#if showEcosystemClear}
      <button
        type="button"
        class="btn btn-secondary"
        on:click={clearEcosystem}>Clear ecosystem</button
      >
    {/if}
    {#if showPackageClear}
      <button type="button" class="btn btn-secondary" on:click={clearPackage}
        >Clear package</button
      >
    {/if}
  </div>
  <p class="settings-copy" style="margin-top:0.75rem; margin-bottom:0;">
    {summary}
  </p>
</form>
