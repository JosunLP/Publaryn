<script lang="ts">
  import type { TeamPackageAccessGrant } from '../../api/orgs';
  import type {
    TeamAccessGrantTargetOption,
    TeamAccessPermissionOption,
  } from '../../pages/team-management';
  import {
    buildPackageDetailsPath,
    buildPackageSecurityPath,
  } from '../../pages/package-security-links';
  import { formatTeamPermission } from '../../pages/team-management';
  import TeamAccessGrantForm from './TeamAccessGrantForm.svelte';
  import { ecosystemLabel } from '../../utils/ecosystem';
  import { formatDate } from '../../utils/format';

  export let grants: TeamPackageAccessGrant[] = [];
  export let grantsError: string | null = null;
  export let optionsError: string | null = null;
  export let options: readonly TeamAccessGrantTargetOption[] = [];
  export let permissionOptions: readonly TeamAccessPermissionOption[] = [];
  export let fieldId = 'team-package-access';
  export let emptyGrantsMessage = 'No package grants are assigned to this team yet.';
  export let handleSubmit: (event: SubmitEvent) => void | Promise<void> = () => {};
  export let handleRevoke: (
    ecosystem: string,
    packageName: string
  ) => void | Promise<void> = () => {};
</script>

{#if grantsError}
  <div class="alert alert-error">{grantsError}</div>
{:else if grants.length === 0}
  <div class="empty-state">
    <p>{emptyGrantsMessage}</p>
  </div>
{:else}
  <div class="token-list">
    {#each grants as grant}
      {@const packageSecurityPath = buildPackageSecurityPath(
        grant.ecosystem || 'unknown',
        grant.name || ''
      )}
      {@const packageDetailsPath = buildPackageDetailsPath(
        grant.ecosystem || 'unknown',
        grant.name || ''
      )}
      <div class="token-row">
        <div class="token-row__main">
          <div class="token-row__title">
            <a
              href={packageSecurityPath}
              data-sveltekit-preload-data="hover">{grant.name || 'Unnamed package'}</a
            >
          </div>
          <div class="token-row__meta">
            <span>{ecosystemLabel(grant.ecosystem)}</span>
            <span>Granted {formatDate(grant.granted_at)}</span>
          </div>
          <div class="token-row__scopes">
            {#each grant.permissions || [] as permission}
              <span class="badge badge-ecosystem">{formatTeamPermission(permission)}</span>
            {/each}
          </div>
        </div>
        {#if grant.ecosystem && grant.name}
          <div class="token-row__actions">
            <a
              href={packageDetailsPath}
              class="btn btn-secondary btn-sm"
              data-sveltekit-preload-data="hover"
            >
              Open package details
            </a>
            <button
              id={`team-package-revoke-${encodeURIComponent(`${grant.ecosystem}-${grant.name}`)}`}
              class="btn btn-secondary btn-sm"
              type="button"
              on:click={() => handleRevoke(grant.ecosystem || '', grant.name || '')}
              >Revoke</button
            >
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<TeamAccessGrantForm
  {fieldId}
  selectLabel="Organization package"
  selectName="package_key"
  placeholderLabel="Select a package"
  emptyMessage="Create or transfer a package before delegating access."
  submitLabel="Save package access"
  error={optionsError}
  {options}
  {permissionOptions}
  handleSubmit={handleSubmit}
/>
