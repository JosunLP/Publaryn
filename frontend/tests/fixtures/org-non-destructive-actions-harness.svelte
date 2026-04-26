<script lang="ts">
  import type { NamespaceClaim } from '../../src/api/namespaces';
  import type { Team } from '../../src/api/orgs';
  import type { OrgPackageSummary, OrgRepositorySummary } from '../../src/api/orgs';
  import {
    createOrgNonDestructiveActionsController,
    type OrgNonDestructiveActionsMutations,
  } from '../../src/pages/org-non-destructive-actions';
  import { ECOSYSTEMS, ecosystemLabel } from '../../src/utils/ecosystem';

  export let slug = 'source-org';
  export let loadState: (options?: {
    notice?: string | null;
    error?: string | null;
  }) => Promise<{
    orgId: string | null;
    teams: Team[];
    namespaces: NamespaceClaim[];
    repositories: OrgRepositorySummary[];
    packages: OrgPackageSummary[];
  }>;
  export let mutations: OrgNonDestructiveActionsMutations | undefined = undefined;

  let notice: string | null = null;
  let error: string | null = null;
  let orgId: string | null = null;
  let teams: Team[] = [];
  let namespaces: NamespaceClaim[] = [];
  let repositories: OrgRepositorySummary[] = [];
  let packages: OrgPackageSummary[] = [];
  let newPackageRepositorySlug = '';
  let newPackageEcosystem = 'npm';
  let newPackageName = '';
  let newPackageVisibility = '';
  let newPackageDisplayName = '';
  let newPackageDescription = '';
  let creatingPackage = false;

  async function reload(
    options: {
      notice?: string | null;
      error?: string | null;
    } = {}
  ): Promise<void> {
    notice = options.notice ?? null;
    error = options.error ?? null;
    const state = await loadState(options);
    orgId = state.orgId;
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

  $: if (repositories.length === 0) {
    if (newPackageRepositorySlug !== '') {
      newPackageRepositorySlug = '';
    }
  } else if (
    !repositories.some((repository) => repository.slug === newPackageRepositorySlug)
  ) {
    newPackageRepositorySlug = repositories[0]?.slug || '';
  }

  const controller = createOrgNonDestructiveActionsController({
    getOrgSlug: () => slug,
    getOrgId: () => orgId,
    reload,
    toErrorMessage,
    ecosystemLabel,
    clearFlash: () => {
      notice = null;
      error = null;
    },
    setError: (value) => {
      error = value;
    },
    setCreatingPackage: (value) => {
      creatingPackage = value;
    },
    getCreatableRepositoriesCount: () => repositories.length,
    resolvePackageCreationRepository: (repositorySlug) => {
      const repository = repositories.find(
        (entry) => entry.slug === repositorySlug
      );
      return repository?.slug
        ? {
            slug: repository.slug,
            name: repository.name,
            visibility: repository.visibility,
          }
        : null;
    },
    resetPackageDraft: () => {
      newPackageEcosystem = 'npm';
      newPackageName = '';
      newPackageVisibility = '';
      newPackageDisplayName = '';
      newPackageDescription = '';
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
  <form id="team-create-form" on:submit={(event) => controller.submitTeamCreate(event)}>
    <input id="team-create-name" name="name" />
    <input id="team-create-slug" name="team_slug" />
    <textarea id="team-create-description" name="description"></textarea>
    <button type="submit">Create team</button>
  </form>
  {#each teams as team}
    <div data-test={`team-${team.slug || 'unknown'}`}>{team.name || team.slug}</div>
  {/each}
</section>

<section>
  <h2>Namespace claims</h2>
  <form
    id="namespace-create-form"
    on:submit={(event) => controller.submitNamespaceCreate(event)}
  >
    <select id="namespace-ecosystem" name="ecosystem">
      {#each ECOSYSTEMS as ecosystem}
        <option value={ecosystem.id}>{ecosystem.label}</option>
      {/each}
    </select>
    <input id="namespace-value" name="namespace" />
    <button type="submit">Create namespace claim</button>
  </form>
  {#each namespaces as claim}
    <div data-test={`namespace-${claim.id || 'unknown'}`}>{claim.namespace}</div>
  {/each}
</section>

<section>
  <h2>Repositories</h2>
  <form
    id="repository-create-form"
    on:submit={(event) => controller.submitRepositoryCreate(event)}
  >
    <input id="repository-create-name" name="name" />
    <input id="repository-create-slug" name="slug" />
    <select id="repository-create-kind" name="kind">
      <option value="public">Public</option>
      <option value="private">Private</option>
      <option value="release">Release</option>
    </select>
    <select id="repository-create-visibility" name="visibility">
      <option value="public">Public</option>
      <option value="private">Private</option>
      <option value="quarantined">Quarantined</option>
    </select>
    <textarea id="repository-create-description" name="description"></textarea>
    <button type="submit">Create repository</button>
  </form>

  {#each repositories as repository}
    {#if repository.slug}
      <div data-test={`repository-${repository.slug}`}>
        <span>{repository.name || repository.slug}</span>
        <span>{repository.visibility}</span>
        <span>{repository.description}</span>
        <form
          id={`repository-update-form-${repository.slug}`}
          on:submit={(event) =>
            controller.submitRepositoryUpdate(event, repository.slug || '')}
        >
          <select
            id={`repository-visibility-${repository.slug}`}
            name="visibility"
            value={repository.visibility || 'public'}
          >
            <option value="public">Public</option>
            <option value="private">Private</option>
            <option value="quarantined">Quarantined</option>
          </select>
          <textarea
            id={`repository-description-${repository.slug}`}
            name="description"
          >{repository.description || ''}</textarea>
          <button type="submit">Save repository</button>
        </form>
      </div>
    {/if}
  {/each}
</section>

<section>
  <h2>Packages</h2>
  <form id="package-create-form" on:submit={(event) => controller.submitPackageCreate(event)}>
    <select
      id="package-create-repository"
      name="repository_slug"
      bind:value={newPackageRepositorySlug}
    >
      {#each repositories as repository}
        {#if repository.slug}
          <option value={repository.slug}>{repository.name || repository.slug}</option>
        {/if}
      {/each}
    </select>
    <select
      id="package-create-ecosystem"
      name="ecosystem"
      bind:value={newPackageEcosystem}
    >
      {#each ECOSYSTEMS as ecosystem}
        <option value={ecosystem.id}>{ecosystem.label}</option>
      {/each}
    </select>
    <input id="package-create-name" name="name" bind:value={newPackageName} />
    <input
      id="package-create-display-name"
      name="display_name"
      bind:value={newPackageDisplayName}
    />
    <select
      id="package-create-visibility"
      name="visibility"
      bind:value={newPackageVisibility}
    >
      <option value="">Default</option>
      <option value="public">Public</option>
      <option value="private">Private</option>
      <option value="quarantined">Quarantined</option>
    </select>
    <textarea
      id="package-create-description"
      name="description"
      bind:value={newPackageDescription}
    ></textarea>
    <button type="submit">{creatingPackage ? 'Creating…' : 'Create package'}</button>
  </form>
  {#each packages as pkg}
    <div data-test={`package-${pkg.name || 'unknown'}`}>{pkg.name}</div>
  {/each}
</section>
