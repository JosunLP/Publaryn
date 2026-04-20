<script lang="ts">
  import { page } from '$app/stores';

  import { ApiError } from '../../../api/client';
  import { createPackage } from '../../../api/packages';
  import type {
    RepositoryDetail,
    RepositoryPackageSummary,
  } from '../../../api/repositories';
  import {
    getRepository,
    listRepositoryPackages,
    updateRepository,
  } from '../../../api/repositories';
  import {
    ECOSYSTEMS,
    ecosystemIcon,
    ecosystemLabel,
  } from '../../../utils/ecosystem';
  import { formatDate, formatNumber } from '../../../utils/format';
  import {
    REPOSITORY_VISIBILITY_OPTIONS,
    formatRepositoryKindLabel,
    formatRepositoryPackageCoverageLabel,
    formatRepositoryVisibilityLabel,
    resolveRepositoryOwnerSummary,
  } from '../../../utils/repositories';
  import { deriveRepositoryDetailCapabilities } from '../../../utils/repository-detail';

  const MAX_VISIBLE_PACKAGES = 100;
  const DEFAULT_PACKAGE_ECOSYSTEM = 'npm';

  let lastSlug = '';
  let loading = true;
  let loadError: string | null = null;
  let notice: string | null = null;
  let error: string | null = null;
  let repository: RepositoryDetail | null = null;
  let packages: RepositoryPackageSummary[] = [];
  let packageError: string | null = null;
  let notFound = false;

  let updatingRepository = false;
  let repositoryDescription = '';
  let repositoryVisibility = 'public';
  let repositoryUpstreamUrl = '';

  let creatingPackage = false;
  let newPackageEcosystem = DEFAULT_PACKAGE_ECOSYSTEM;
  let newPackageName = '';
  let newPackageDisplayName = '';
  let newPackageDescription = '';
  let newPackageVisibility = '';

  $: slug = $page.params.slug ?? '';
  $: if (slug && slug !== lastSlug) {
    lastSlug = slug;
    void loadRepositoryPage();
  }

  $: repositoryCapabilities = deriveRepositoryDetailCapabilities(repository);
  $: explicitPackageVisibilityOptions =
    repositoryCapabilities.defaultPackageVisibility
      ? repositoryCapabilities.packageVisibilityOptions.filter(
          (option) =>
            option.value !== repositoryCapabilities.defaultPackageVisibility
        )
      : repositoryCapabilities.packageVisibilityOptions;
  $: if (
    newPackageVisibility &&
    !repositoryCapabilities.packageVisibilityOptions.some(
      (option) => option.value === newPackageVisibility
    )
  ) {
    newPackageVisibility = '';
  }

  async function loadRepositoryPage(
    options: { notice?: string | null; error?: string | null } = {}
  ): Promise<void> {
    loading = true;
    loadError = null;
    notice = options.notice ?? null;
    error = options.error ?? null;
    repository = null;
    packages = [];
    packageError = null;
    notFound = false;

    try {
      repository = await getRepository(slug);
      repositoryDescription = repository.description || '';
      repositoryVisibility =
        normalizeRepositoryValue(repository.visibility) || 'public';
      repositoryUpstreamUrl = repository.upstream_url || '';
    } catch (caughtError: unknown) {
      if (caughtError instanceof ApiError && caughtError.status === 404) {
        notFound = true;
      } else {
        loadError = toErrorMessage(caughtError, 'Failed to load repository.');
      }
      loading = false;
      return;
    }

    try {
      const packageData = await listRepositoryPackages(slug, {
        perPage: MAX_VISIBLE_PACKAGES,
      });
      packages = packageData.packages || [];
      packageError = packageData.load_error || null;
    } catch (caughtError: unknown) {
      packageError = toErrorMessage(
        caughtError,
        'Failed to load repository packages.'
      );
    } finally {
      loading = false;
    }
  }

  async function handleRepositoryUpdate(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!repository || repositoryCapabilities.canManage !== true) {
      return;
    }

    updatingRepository = true;
    notice = null;
    error = null;

    try {
      const result = await updateRepository(repository.slug?.trim() || slug, {
        description: normalizeOptionalText(repositoryDescription),
        visibility: normalizeRepositoryValue(repositoryVisibility) || undefined,
        upstreamUrl: normalizeOptionalText(repositoryUpstreamUrl),
      });

      await loadRepositoryPage({
        notice:
          typeof result.message === 'string' && result.message.trim().length > 0
            ? result.message
            : 'Repository updated.',
      });
    } catch (caughtError: unknown) {
      await loadRepositoryPage({
        error: toErrorMessage(caughtError, 'Failed to update repository.'),
      });
    } finally {
      updatingRepository = false;
    }
  }

  async function handleCreatePackage(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!repository || repositoryCapabilities.canCreatePackages !== true) {
      return;
    }

    if (!repositoryCapabilities.packageCreationEligible) {
      await loadRepositoryPage({
        error:
          repositoryCapabilities.packageCreationMessage ||
          'This repository does not support direct package creation.',
      });
      return;
    }

    if (repositoryCapabilities.packageVisibilityOptions.length === 0) {
      notice = null;
      error =
        repositoryCapabilities.packageCreationMessage ||
        'This repository cannot host directly created packages.';
      return;
    }

    const packageName = newPackageName.trim();
    if (!packageName) {
      notice = null;
      error = 'Enter a package name.';
      return;
    }

    const repositorySlug = repository.slug?.trim() || slug;
    const repositoryName =
      repository.name?.trim() || repository.slug?.trim() || 'this repository';
    const ecosystem =
      newPackageEcosystem.trim().toLowerCase() || DEFAULT_PACKAGE_ECOSYSTEM;

    creatingPackage = true;
    notice = null;
    error = null;

    try {
      const result = await createPackage({
        ecosystem,
        name: packageName,
        repositorySlug,
        visibility: newPackageVisibility || undefined,
        displayName: newPackageDisplayName,
        description: newPackageDescription,
      });

      newPackageEcosystem = DEFAULT_PACKAGE_ECOSYSTEM;
      newPackageName = '';
      newPackageDisplayName = '';
      newPackageDescription = '';
      newPackageVisibility = '';

      await loadRepositoryPage({
        notice: `Created ${ecosystemLabel(result.ecosystem || ecosystem)} package ${result.name || packageName} in ${repositoryName}.`,
      });
    } catch (caughtError: unknown) {
      await loadRepositoryPage({
        error: toErrorMessage(caughtError, 'Failed to create package.'),
      });
    } finally {
      creatingPackage = false;
    }
  }

  function formatVisiblePackageSummary(packageCount: number): string {
    if (packageCount >= MAX_VISIBLE_PACKAGES) {
      return `Showing the first ${MAX_VISIBLE_PACKAGES} visible packages.`;
    }

    return formatRepositoryPackageCoverageLabel(packageCount, packageCount);
  }

  function formatFileOwnerSummary() {
    return resolveRepositoryOwnerSummary({
      ownerOrgName: repository?.owner_org_name,
      ownerOrgSlug: repository?.owner_org_slug,
      ownerUsername: repository?.owner_username,
    });
  }

  function normalizeOptionalText(value: string): string | null {
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : null;
  }

  function normalizeRepositoryValue(value: string | null | undefined): string {
    return value?.trim().toLowerCase().replace(/-/g, '_') || '';
  }

  function toErrorMessage(caughtError: unknown, fallback: string): string {
    return caughtError instanceof Error && caughtError.message
      ? caughtError.message
      : fallback;
  }
</script>

<svelte:head>
  <title>Repository details — Publaryn</title>
</svelte:head>

{#if loading}
  <div class="loading"><span class="spinner"></span> Loading…</div>
{:else if notFound}
  <div class="empty-state mt-6">
    <h2>Repository not found</h2>
    <p>@{slug} does not exist or is not visible to you.</p>
    <a
      href="/search"
      class="btn btn-primary mt-4"
      data-sveltekit-preload-data="hover">Search packages</a
    >
  </div>
{:else if loadError || !repository}
  <div class="alert alert-error mt-6">
    Failed to load repository: {loadError || 'Unknown error.'}
  </div>
{:else}
  {@const ownerSummary = formatFileOwnerSummary()}
  {@const repositorySlug = repository.slug?.trim() || slug}
  {@const repositoryName =
    repository.name?.trim() || repositorySlug || 'Repository'}

  <div class="page-shell">
    {#if notice}<div class="alert alert-success mb-4">{notice}</div>{/if}
    {#if error}<div class="alert alert-error mb-4">{error}</div>{/if}

    <nav class="page-breadcrumbs">
      {#if ownerSummary.href}
        <a href={ownerSummary.href} data-sveltekit-preload-data="hover"
          >{ownerSummary.label}</a
        >
        <span>&rsaquo; </span>
      {/if}
      <span>{repositoryName}</span>
    </nav>

    <section class="page-hero">
      <div class="page-hero__header">
        <div class="page-hero__copy">
          <span class="page-hero__eyebrow">
            <span class="page-hero__eyebrow-dot" aria-hidden="true"></span>
            Repository
          </span>
          <h1 class="page-hero__title">{repositoryName}</h1>
          <p class="page-hero__subtitle">
            {repository.description ||
              'A repository-backed package surface with enterprise visibility, ownership, and creation controls.'}
          </p>
          <div class="page-hero__meta">
            <span class="badge badge-ecosystem">@{repositorySlug}</span>
            <span class="badge badge-ecosystem"
              >{formatRepositoryKindLabel(repository.kind)}</span
            >
            <span class="badge badge-ecosystem"
              >{formatRepositoryVisibilityLabel(repository.visibility)}</span
            >
          </div>
        </div>
      </div>
    </section>

    <div class="page-stats">
      <div class="page-stat">
        <div class="page-stat__label">Visible packages</div>
        <div class="page-stat__value">{formatNumber(packages.length)}</div>
      </div>
      <div class="page-stat">
        <div class="page-stat__label">Owner</div>
        <div class="page-stat__value">{ownerSummary.label}</div>
      </div>
      <div class="page-stat">
        <div class="page-stat__label">Kind</div>
        <div class="page-stat__value">{formatRepositoryKindLabel(repository.kind)}</div>
      </div>
      <div class="page-stat">
        <div class="page-stat__label">Visibility</div>
        <div class="page-stat__value">
          {formatRepositoryVisibilityLabel(repository.visibility)}
        </div>
      </div>
    </div>

    <div class="detail-grid">
      <div class="detail-main">
        <section class="surface-card">
          <div class="surface-card__header">
            <div class="surface-card__title">Visible packages</div>
            <p class="surface-card__copy">
              {formatVisiblePackageSummary(packages.length)}
            </p>
          </div>

          <div class="surface-card__body" style="padding-top:0;">
          {#if packageError}
            <div class="alert alert-error">{packageError}</div>
          {:else if packages.length === 0}
            <div class="empty-state">
              <p>No visible packages belong to this repository yet.</p>
            </div>
          {:else}
            {#each packages as pkg}
              <div class="release-row">
                <div>
                  <a
                    href={`/packages/${encodeURIComponent(pkg.ecosystem || 'unknown')}/${encodeURIComponent(pkg.name || '')}`}
                    class="release-row__version"
                    data-sveltekit-preload-data="hover"
                  >
                    {ecosystemIcon(pkg.ecosystem)}
                    {pkg.name || 'Unnamed package'}
                  </a>
                  <span class="text-muted">
                    {ecosystemLabel(pkg.ecosystem)}
                  </span>
                  {#if pkg.visibility}
                    <span class="badge badge-ecosystem"
                      >{formatRepositoryVisibilityLabel(pkg.visibility)}</span
                    >
                  {/if}
                  {#if pkg.description}
                    <div class="settings-copy mt-4">
                      {pkg.description}
                    </div>
                  {/if}
                </div>
                <div class="release-row__meta">
                  {#if pkg.download_count != null}{formatNumber(
                      pkg.download_count
                    )} downloads{/if}
                  {#if pkg.created_at}
                    {pkg.download_count != null ? ' · ' : ''}created {formatDate(
                      pkg.created_at
                    )}
                  {/if}
                </div>
              </div>
            {/each}
          {/if}
          </div>
        </section>

        {#if repositoryCapabilities.canManage}
          <section class="surface-card settings-section">
            <div class="surface-card__header">
              <h2 class="surface-card__title">Repository settings</h2>
              <p class="surface-card__copy">
                Update the repository description, visibility, and upstream metadata.
              </p>
            </div>

            <form class="surface-card__body" on:submit={handleRepositoryUpdate}>
              <div class="grid gap-4 xl:grid-cols-2">
                <div class="form-group">
                  <label for="repository-kind">Repository kind</label>
                  <input
                    id="repository-kind"
                    class="form-input"
                    value={formatRepositoryKindLabel(repository.kind)}
                    disabled
                  />
                </div>

                <div class="form-group">
                  <label for="repository-visibility">Visibility</label>
                  <select
                    id="repository-visibility"
                    name="visibility"
                    class="form-input"
                    bind:value={repositoryVisibility}
                  >
                    {#each REPOSITORY_VISIBILITY_OPTIONS as option}
                      <option value={option.value}>{option.label}</option>
                    {/each}
                  </select>
                </div>
              </div>

              <div class="form-group">
                <label for="repository-upstream">Upstream URL</label>
                <input
                  id="repository-upstream"
                  name="upstream_url"
                  class="form-input"
                  type="url"
                  bind:value={repositoryUpstreamUrl}
                  placeholder="https://registry.npmjs.org"
                />
              </div>

              <div class="form-group">
                <label for="repository-description">Description</label>
                <textarea
                  id="repository-description"
                  name="description"
                  class="form-input"
                  rows="3"
                  bind:value={repositoryDescription}
                ></textarea>
              </div>

              <button
                type="submit"
                class="btn btn-primary"
                disabled={updatingRepository}
              >
                {updatingRepository ? 'Saving…' : 'Save repository'}
              </button>
            </form>
          </section>
        {/if}

        {#if repositoryCapabilities.showPackageCreationSection}
          <section class="surface-card settings-section">
            <div class="surface-card__header">
              <h2 class="surface-card__title">Create a package</h2>
              <p class="surface-card__copy">
                Package ownership is derived from this repository. Visibility
                cannot be broader than the repository visibility, and matching
                namespace claims currently constrain npm/Bun scopes, Composer
                vendors, and Maven group IDs.
              </p>
            </div>

            {#if repositoryCapabilities.packageCreationMessage}
              <div class="alert alert-warning surface-card__body" style="padding-top:0;">
                {repositoryCapabilities.packageCreationMessage}
              </div>
            {/if}

            {#if repositoryCapabilities.canCreatePackages && repositoryCapabilities.packageCreationEligible && repositoryCapabilities.packageVisibilityOptions.length > 0}
              <form class="surface-card__body" on:submit={handleCreatePackage}>
                <div class="grid gap-4 xl:grid-cols-2">
                  <div class="form-group">
                    <label for="package-create-ecosystem">Ecosystem</label>
                    <select
                      id="package-create-ecosystem"
                      name="ecosystem"
                      class="form-input"
                      bind:value={newPackageEcosystem}
                    >
                      {#each ECOSYSTEMS as ecosystem}
                        <option value={ecosystem.id}>{ecosystem.label}</option>
                      {/each}
                    </select>
                  </div>

                  <div class="form-group">
                    <label for="package-create-visibility">Visibility</label>
                    <select
                      id="package-create-visibility"
                      name="visibility"
                      class="form-input"
                      bind:value={newPackageVisibility}
                    >
                      <option value="">
                        Use repository default ({formatRepositoryVisibilityLabel(
                          repository.visibility
                        )})
                      </option>
                      {#each explicitPackageVisibilityOptions as option}
                        <option value={option.value}>{option.label}</option>
                      {/each}
                    </select>
                  </div>
                </div>

                <div class="form-group">
                  <label for="package-create-name">Package name</label>
                  <input
                    id="package-create-name"
                    name="name"
                    class="form-input"
                    bind:value={newPackageName}
                    placeholder="acme-widget, @acme/widget, acme/widget, com.acme:artifact"
                    required
                  />
                </div>

                <div class="grid gap-4 xl:grid-cols-2">
                  <div class="form-group">
                    <label for="package-create-display-name">Display name</label
                    >
                    <input
                      id="package-create-display-name"
                      name="display_name"
                      class="form-input"
                      bind:value={newPackageDisplayName}
                      placeholder="Optional friendly title"
                    />
                  </div>

                  <div class="form-group">
                    <label for="package-create-repository">Repository</label>
                    <input
                      id="package-create-repository"
                      class="form-input"
                      value={`${repositoryName} (@${repositorySlug})`}
                      disabled
                    />
                  </div>
                </div>

                <div class="form-group">
                  <label for="package-create-description">Description</label>
                  <textarea
                    id="package-create-description"
                    name="description"
                    class="form-input"
                    rows="3"
                    bind:value={newPackageDescription}
                    placeholder="Optional package summary"
                  ></textarea>
                </div>

                <p class="settings-copy">
                  New packages created here will inherit ownership from
                  <strong>{repositoryName}</strong> and stay within
                  <strong
                    >{formatRepositoryVisibilityLabel(
                      repository.visibility
                    )}</strong
                  >
                  visibility constraints.
                </p>

                <button
                  type="submit"
                  class="btn btn-primary"
                  disabled={creatingPackage}
                >
                  {creatingPackage ? 'Creating…' : 'Create package'}
                </button>
              </form>
            {/if}
          </section>
        {/if}
      </div>

      <div class="detail-sidebar">
        <div class="card">
          <div class="sidebar-section">
            <h3>Repository info</h3>
            <div class="sidebar-row">
              <span class="sidebar-row__label">Slug</span><span
                class="sidebar-row__value">@{repositorySlug}</span
              >
            </div>
            <div class="sidebar-row">
              <span class="sidebar-row__label">Owner</span><span
                class="sidebar-row__value"
                >{#if ownerSummary.href}<a
                    href={ownerSummary.href}
                    data-sveltekit-preload-data="hover">{ownerSummary.label}</a
                  >{:else}{ownerSummary.label}{/if}</span
              >
            </div>
            <div class="sidebar-row">
              <span class="sidebar-row__label">Kind</span><span
                class="sidebar-row__value"
                >{formatRepositoryKindLabel(repository.kind)}</span
              >
            </div>
            <div class="sidebar-row">
              <span class="sidebar-row__label">Visibility</span><span
                class="sidebar-row__value"
                >{formatRepositoryVisibilityLabel(repository.visibility)}</span
              >
            </div>
            <div class="sidebar-row">
              <span class="sidebar-row__label">Visible packages</span><span
                class="sidebar-row__value">{formatNumber(packages.length)}</span
              >
            </div>
            {#if repository.created_at}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Created</span><span
                  class="sidebar-row__value"
                  >{formatDate(repository.created_at)}</span
                >
              </div>
            {/if}
            {#if repository.updated_at}
              <div class="sidebar-row">
                <span class="sidebar-row__label">Updated</span><span
                  class="sidebar-row__value"
                  >{formatDate(repository.updated_at)}</span
                >
              </div>
            {/if}
          </div>
        </div>

        {#if repository.upstream_url}
          <div class="card">
            <div class="sidebar-section">
              <h3>Upstream</h3>
              <a
                href={repository.upstream_url}
                target="_blank"
                rel="noopener noreferrer">{repository.upstream_url}</a
              >
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}
