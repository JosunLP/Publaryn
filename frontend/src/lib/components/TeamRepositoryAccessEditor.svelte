<script lang="ts">
  import type { TeamRepositoryAccessGrant } from '../../api/orgs';
  import type {
    TeamAccessGrantTargetOption,
    TeamAccessPermissionOption,
  } from '../../pages/team-management';
  import { formatTeamPermission } from '../../pages/team-management';
  import TeamAccessGrantForm from './TeamAccessGrantForm.svelte';
  import { formatDate } from '../../utils/format';
  import {
    formatRepositoryKindLabel,
    formatRepositoryVisibilityLabel,
  } from '../../utils/repositories';

  export let grants: TeamRepositoryAccessGrant[] = [];
  export let grantsError: string | null = null;
  export let optionsError: string | null = null;
  export let options: readonly TeamAccessGrantTargetOption[] = [];
  export let permissionOptions: readonly TeamAccessPermissionOption[] = [];
  export let fieldId = 'team-repository-access';
  export let emptyGrantsMessage =
    'No repository grants are assigned to this team yet.';
  export let handleSubmit: (event: SubmitEvent) => void | Promise<void> = () => {};
  export let handleRevoke: (repositorySlug: string) => void | Promise<void> = () => {};
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
            {#if grant.slug}
              <a
                href={`/repositories/${encodeURIComponent(grant.slug)}`}
                data-sveltekit-preload-data="hover">{grant.name || grant.slug}</a
              >
            {:else}
              {grant.name || 'Unnamed repository'}
            {/if}
          </div>
          <div class="token-row__meta">
            <span>@{grant.slug || 'no-slug'}</span>
            <span>{formatRepositoryKindLabel(grant.kind)}</span>
            <span>{formatRepositoryVisibilityLabel(grant.visibility)}</span>
            <span>Granted {formatDate(grant.granted_at)}</span>
          </div>
          <div class="token-row__scopes">
            {#each grant.permissions || [] as permission}
              <span class="badge badge-ecosystem">{formatTeamPermission(permission)}</span>
            {/each}
          </div>
        </div>
        {#if grant.slug}
          <div class="token-row__actions">
            <button
              id={`team-repository-revoke-${encodeURIComponent(grant.slug)}`}
              class="btn btn-secondary btn-sm"
              type="button"
              on:click={() => handleRevoke(grant.slug || '')}>Revoke</button
            >
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<TeamAccessGrantForm
  {fieldId}
  selectLabel="Organization repository"
  selectName="repository_slug"
  placeholderLabel="Select a repository"
  emptyMessage="Create a repository before delegating repository-wide access."
  submitLabel="Save repository access"
  error={optionsError}
  {options}
  {permissionOptions}
  handleSubmit={handleSubmit}
/>
