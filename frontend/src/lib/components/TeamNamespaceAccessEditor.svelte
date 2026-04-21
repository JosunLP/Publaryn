<script lang="ts">
  import type { TeamNamespaceAccessGrant } from '../../api/orgs';
  import type {
    TeamAccessGrantTargetOption,
    TeamAccessPermissionOption,
  } from '../../pages/team-management';
  import { formatTeamPermission } from '../../pages/team-management';
  import TeamAccessGrantForm from './TeamAccessGrantForm.svelte';
  import { ecosystemLabel } from '../../utils/ecosystem';
  import { formatDate } from '../../utils/format';

  export let grants: TeamNamespaceAccessGrant[] = [];
  export let grantsError: string | null = null;
  export let optionsError: string | null = null;
  export let options: readonly TeamAccessGrantTargetOption[] = [];
  export let permissionOptions: readonly TeamAccessPermissionOption[] = [];
  export let fieldId = 'team-namespace-access';
  export let emptyGrantsMessage =
    'No namespace grants are assigned to this team yet.';
  export let handleSubmit: (event: SubmitEvent) => void | Promise<void> = () => {};
  export let handleRevoke: (
    claimId: string,
    namespace: string
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
      <div class="token-row">
        <div class="token-row__main">
          <div class="token-row__title">
            {grant.namespace || 'Unnamed namespace claim'}
          </div>
          <div class="token-row__meta">
            <span>{ecosystemLabel(grant.ecosystem)}</span>
            <span>{grant.is_verified ? 'verified' : 'pending verification'}</span>
            <span>Granted {formatDate(grant.granted_at)}</span>
          </div>
          <div class="token-row__scopes">
            {#each grant.permissions || [] as permission}
              <span class="badge badge-ecosystem">{formatTeamPermission(permission)}</span>
            {/each}
          </div>
        </div>
        {#if grant.namespace_claim_id}
          <div class="token-row__actions">
            <button
              id={`team-namespace-revoke-${encodeURIComponent(grant.namespace_claim_id)}`}
              class="btn btn-secondary btn-sm"
              type="button"
              on:click={() =>
                handleRevoke(
                  grant.namespace_claim_id || '',
                  grant.namespace || 'this namespace claim'
                )}>Revoke</button
            >
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<TeamAccessGrantForm
  {fieldId}
  selectLabel="Organization namespace claim"
  selectName="claim_id"
  placeholderLabel="Select a namespace claim"
  emptyMessage="Create or transfer a namespace claim before delegating access."
  submitLabel="Save namespace access"
  error={optionsError}
  {options}
  {permissionOptions}
  handleSubmit={handleSubmit}
/>
