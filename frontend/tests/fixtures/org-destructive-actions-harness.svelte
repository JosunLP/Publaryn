<script lang="ts">
  import type { NamespaceClaim } from '../../src/api/namespaces';
  import type { Team } from '../../src/api/orgs';
  import type { OrgPackageSummary, OrgRepositorySummary } from '../../src/api/orgs';
  import {
    createOrgDestructiveActionsController,
    type OrgDestructiveActionsMutations,
  } from '../../src/pages/org-destructive-actions';
  import { renderPackageSelectionValue } from '../../src/pages/org-workspace-actions';

  export let slug = 'source-org';
  export let transferTargets: string[] = ['target-org'];
  export let loadState: (options?: {
    notice?: string | null;
    error?: string | null;
  }) => Promise<{
    teams: Team[];
    namespaces: NamespaceClaim[];
    repositories: OrgRepositorySummary[];
    packages: OrgPackageSummary[];
  }>;
  export let mutations: OrgDestructiveActionsMutations | undefined = undefined;

  let notice: string | null = null;
  let error: string | null = null;
  let teams: Team[] = [];
  let namespaces: NamespaceClaim[] = [];
  let repositories: OrgRepositorySummary[] = [];
  let packages: OrgPackageSummary[] = [];
  let teamDeleteTargetSlug: string | null = null;
  let teamDeleteConfirmed = false;
  let deletingTeamSlug: string | null = null;
  let namespaceDeleteTargetId: string | null = null;
  let namespaceDeleteConfirmed = false;
  let deletingNamespaceClaimId: string | null = null;
  let namespaceTransferConfirmationOpen = false;
  let namespaceTransferConfirmed = false;
  let transferringNamespaceOwnership = false;
  let selectedNamespaceTransferClaimId = '';
  let selectedNamespaceTransferTargetOrgSlug = '';
  let repositoryTransferConfirmationOpen = false;
  let repositoryTransferConfirmed = false;
  let transferringRepositoryOwnership = false;
  let selectedRepositoryTransferSlug = '';
  let selectedRepositoryTransferTargetOrgSlug = '';
  let packageTransferConfirmationOpen = false;
  let packageTransferConfirmed = false;
  let transferringPackageOwnershipFlow = false;
  let selectedPackageTransferKey = '';
  let selectedPackageTransferTargetOrgSlug = '';

  async function reload(
    options: {
      notice?: string | null;
      error?: string | null;
    } = {}
  ): Promise<void> {
    notice = options.notice ?? null;
    error = options.error ?? null;
    const state = await loadState(options);
    teams = state.teams;
    namespaces = state.namespaces;
    repositories = state.repositories;
    packages = state.packages;
  }

  function toErrorMessage(caughtError: unknown, fallback: string): string {
    return caughtError instanceof Error && caughtError.message
      ? caughtError.message
      : fallback;
  }

  const controller = createOrgDestructiveActionsController({
    getOrgSlug: () => slug,
    reload,
    toErrorMessage,
    clearFlash: () => {
      notice = null;
      error = null;
    },
    setError: (value) => {
      error = value;
    },
    setTeamDeleteTargetSlug: (value) => {
      teamDeleteTargetSlug = value;
    },
    getTeamDeleteConfirmed: () => teamDeleteConfirmed,
    setTeamDeleteConfirmed: (value) => {
      teamDeleteConfirmed = value;
    },
    setDeletingTeamSlug: (value) => {
      deletingTeamSlug = value;
    },
    setNamespaceDeleteTargetId: (value) => {
      namespaceDeleteTargetId = value;
    },
    getNamespaceDeleteConfirmed: () => namespaceDeleteConfirmed,
    setNamespaceDeleteConfirmed: (value) => {
      namespaceDeleteConfirmed = value;
    },
    setDeletingNamespaceClaimId: (value) => {
      deletingNamespaceClaimId = value;
    },
    setNamespaceTransferConfirmationOpen: (value) => {
      namespaceTransferConfirmationOpen = value;
    },
    getNamespaceTransferConfirmed: () => namespaceTransferConfirmed,
    setNamespaceTransferConfirmed: (value) => {
      namespaceTransferConfirmed = value;
    },
    setTransferringNamespaceOwnership: (value) => {
      transferringNamespaceOwnership = value;
    },
    resolveNamespaceLabel: (claimId) =>
      namespaces.find((claim) => claim.id === claimId)?.namespace || null,
    setRepositoryTransferConfirmationOpen: (value) => {
      repositoryTransferConfirmationOpen = value;
    },
    getRepositoryTransferConfirmed: () => repositoryTransferConfirmed,
    setRepositoryTransferConfirmed: (value) => {
      repositoryTransferConfirmed = value;
    },
    setTransferringRepositoryOwnership: (value) => {
      transferringRepositoryOwnership = value;
    },
    setPackageTransferConfirmationOpen: (value) => {
      packageTransferConfirmationOpen = value;
    },
    getPackageTransferConfirmed: () => packageTransferConfirmed,
    setPackageTransferConfirmed: (value) => {
      packageTransferConfirmed = value;
    },
    setTransferringPackageOwnershipFlow: (value) => {
      transferringPackageOwnershipFlow = value;
    },
    mutations,
  });

  queueMicrotask(() => {
    void reload();
  });
</script>

{#if notice}<div class="alert alert-success">{notice}</div>{/if}
{#if error}<div class="alert alert-error">{error}</div>{/if}

<section>
  <h2>Teams</h2>
  {#each teams as team}
    <div data-test={`team-${team.slug || 'unknown'}`}>
      <span>{team.name || team.slug}</span>
      {#if teamDeleteTargetSlug === team.slug}
        <form
          id={`team-delete-form-${team.slug || 'team'}`}
          on:submit={(event) =>
            controller.submitTeamDelete(event, team.slug || '')}
        >
          <input
            id={`team-delete-confirm-${team.slug || 'team'}`}
            bind:checked={teamDeleteConfirmed}
            type="checkbox"
          />
          <button type="submit">
            {deletingTeamSlug === team.slug ? 'Deleting…' : 'Delete team'}
          </button>
          <button type="button" on:click={controller.cancelTeamDeleteConfirmation}>
            Keep team
          </button>
        </form>
      {:else}
        <button
          id={`team-delete-toggle-${team.slug || 'team'}`}
          type="button"
          on:click={() => controller.openTeamDeleteConfirmation(team.slug || '')}
        >
          Delete…
        </button>
      {/if}
    </div>
  {/each}
</section>

<section>
  <h2>Namespace claims</h2>
  {#each namespaces as claim}
    <div data-test={`namespace-${claim.id || 'unknown'}`}>
      <span>{claim.namespace}</span>
      {#if namespaceDeleteTargetId === claim.id}
        <form
          id={`namespace-delete-form-${claim.id || 'claim'}`}
          on:submit={(event) =>
            controller.submitNamespaceDelete(
              event,
              claim.id,
              claim.namespace || 'namespace claim'
            )}
        >
          <input
            id={`namespace-delete-confirm-${claim.id || 'claim'}`}
            bind:checked={namespaceDeleteConfirmed}
            type="checkbox"
          />
          <button type="submit">
            {deletingNamespaceClaimId === claim.id
              ? 'Deleting…'
              : 'Delete namespace'}
          </button>
          <button
            type="button"
            on:click={controller.cancelNamespaceDeleteConfirmation}
          >
            Keep namespace
          </button>
        </form>
      {:else}
        <button
          id={`namespace-delete-toggle-${claim.id || 'claim'}`}
          type="button"
          on:click={() =>
            controller.openNamespaceDeleteConfirmation(claim.id || '')}
        >
          Delete…
        </button>
      {/if}
    </div>
  {/each}
</section>

<section>
  <h2>Transfer a namespace</h2>
  <form on:submit={(event) => controller.submitNamespaceTransfer(event)}>
    <input
      id="org-namespace-transfer-claim"
      name="claim_id"
      bind:value={selectedNamespaceTransferClaimId}
      list="org-namespace-transfer-claim-options"
    />
    <datalist id="org-namespace-transfer-claim-options">
      {#each namespaces as claim}
        <option value={claim.id || ''}>{claim.namespace}</option>
      {/each}
    </datalist>
    <input
      id="org-namespace-transfer-target"
      name="target_org_slug"
      bind:value={selectedNamespaceTransferTargetOrgSlug}
      list="org-namespace-transfer-target-options"
    />
    <datalist id="org-namespace-transfer-target-options">
      {#each transferTargets as targetOrgSlug}
        <option value={targetOrgSlug}>{targetOrgSlug}</option>
      {/each}
    </datalist>
    {#if namespaceTransferConfirmationOpen}
      <input
        id="org-namespace-transfer-confirm"
        bind:checked={namespaceTransferConfirmed}
        type="checkbox"
      />
      <button id="org-namespace-transfer-submit" type="submit">
        {transferringNamespaceOwnership ? 'Transferring…' : 'Transfer namespace'}
      </button>
      <button type="button" on:click={controller.cancelNamespaceTransferConfirmation}>
        Keep namespace
      </button>
    {:else}
      <button
        id="org-namespace-transfer-toggle"
        type="button"
        on:click={controller.openNamespaceTransferConfirmation}
      >
        Transfer…
      </button>
    {/if}
  </form>
</section>

<section>
  <h2>Transfer repository ownership</h2>
  <form on:submit={(event) => controller.submitRepositoryTransfer(event)}>
    <input
      id="org-repository-transfer-repository"
      name="repository_slug"
      bind:value={selectedRepositoryTransferSlug}
      list="org-repository-transfer-repository-options"
    />
    <datalist id="org-repository-transfer-repository-options">
      {#each repositories as repository}
        <option value={repository.slug || ''}>
          {repository.name || repository.slug}
        </option>
      {/each}
    </datalist>
    <input
      id="org-repository-transfer-target"
      name="target_org_slug"
      bind:value={selectedRepositoryTransferTargetOrgSlug}
      list="org-repository-transfer-target-options"
    />
    <datalist id="org-repository-transfer-target-options">
      {#each transferTargets as targetOrgSlug}
        <option value={targetOrgSlug}>{targetOrgSlug}</option>
      {/each}
    </datalist>
    {#if repositoryTransferConfirmationOpen}
      <input
        id="org-repository-transfer-confirm"
        bind:checked={repositoryTransferConfirmed}
        type="checkbox"
      />
      <button id="org-repository-transfer-submit" type="submit">
        {transferringRepositoryOwnership ? 'Transferring…' : 'Transfer repository'}
      </button>
      <button type="button" on:click={controller.cancelRepositoryTransferConfirmation}>
        Keep repository
      </button>
    {:else}
      <button
        id="org-repository-transfer-toggle"
        type="button"
        on:click={controller.openRepositoryTransferConfirmation}
      >
        Transfer…
      </button>
    {/if}
  </form>
</section>

<section>
  <h2>Transfer package ownership</h2>
  <form on:submit={(event) => controller.submitPackageTransfer(event)}>
    <input
      id="org-package-transfer-package"
      name="package_key"
      bind:value={selectedPackageTransferKey}
      list="org-package-transfer-package-options"
    />
    <datalist id="org-package-transfer-package-options">
      {#each packages as pkg}
        <option value={renderPackageSelectionValue(pkg.ecosystem, pkg.name)}>
          {pkg.name}
        </option>
      {/each}
    </datalist>
    <input
      id="org-package-transfer-target"
      name="target_org_slug"
      bind:value={selectedPackageTransferTargetOrgSlug}
      list="org-package-transfer-target-options"
    />
    <datalist id="org-package-transfer-target-options">
      {#each transferTargets as targetOrgSlug}
        <option value={targetOrgSlug}>{targetOrgSlug}</option>
      {/each}
    </datalist>
    {#if packageTransferConfirmationOpen}
      <input
        id="org-package-transfer-confirm"
        bind:checked={packageTransferConfirmed}
        type="checkbox"
      />
      <button id="org-package-transfer-submit" type="submit">
        {transferringPackageOwnershipFlow ? 'Transferring…' : 'Transfer package'}
      </button>
      <button type="button" on:click={controller.cancelPackageTransferConfirmation}>
        Keep package
      </button>
    {:else}
      <button
        id="org-package-transfer-toggle"
        type="button"
        on:click={controller.openPackageTransferConfirmation}
      >
        Transfer…
      </button>
    {/if}
  </form>
</section>
