<script lang="ts">
  import { page } from '$app/stores';

  import { ApiError } from '../../../api/client';
  import type {
    RepositoryDetail,
    RepositoryPackageSummary,
  } from '../../../api/repositories';
  import {
    getRepository,
    listRepositoryPackages,
  } from '../../../api/repositories';
  import { ecosystemIcon, ecosystemLabel } from '../../../utils/ecosystem';
  import { formatDate, formatNumber } from '../../../utils/format';
  import {
    formatRepositoryKindLabel,
    formatRepositoryPackageCoverageLabel,
    formatRepositoryVisibilityLabel,
    resolveRepositoryOwnerSummary,
  } from '../../../utils/repositories';

  const MAX_VISIBLE_PACKAGES = 100;

  let lastSlug = '';
  let loading = true;
  let loadError: string | null = null;
  let repository: RepositoryDetail | null = null;
  let packages: RepositoryPackageSummary[] = [];
  let packageError: string | null = null;
  let notFound = false;

  $: slug = $page.params.slug ?? '';
  $: if (slug && slug !== lastSlug) {
    lastSlug = slug;
    void loadRepository();
  }

  async function loadRepository(): Promise<void> {
    loading = true;
    loadError = null;
    repository = null;
    packages = [];
    packageError = null;
    notFound = false;

    try {
      repository = await getRepository(slug);
    } catch (caughtError: unknown) {
      if (caughtError instanceof ApiError && caughtError.status === 404) {
        notFound = true;
      } else {
        loadError =
          caughtError instanceof Error && caughtError.message
            ? caughtError.message
            : 'Failed to load repository.';
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
      packageError =
        caughtError instanceof Error && caughtError.message
          ? caughtError.message
          : 'Failed to load repository packages.';
    } finally {
      loading = false;
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

  <div class="mt-6">
    <nav style="font-size:0.875rem; margin-bottom:16px;">
      {#if ownerSummary.href}
        <a href={ownerSummary.href} data-sveltekit-preload-data="hover"
          >{ownerSummary.label}</a
        >
        <span>&rsaquo; </span>
      {/if}
      <span style="color:var(--color-text-secondary);">{repositoryName}</span>
    </nav>

    <div class="pkg-header">
      <h1 class="pkg-header__name">{repositoryName}</h1>
      <span class="badge badge-ecosystem">Repository</span>
      <span class="badge badge-ecosystem"
        >{formatRepositoryKindLabel(repository.kind)}</span
      >
      <span class="badge badge-ecosystem"
        >{formatRepositoryVisibilityLabel(repository.visibility)}</span
      >
    </div>

    <p class="text-muted mt-4" style="font-size:1.05rem;">@{repositorySlug}</p>
    {#if repository.description}
      <p class="text-muted mt-4" style="font-size:1.05rem;">
        {repository.description}
      </p>
    {/if}

    <div class="pkg-detail">
      <div class="pkg-detail__main">
        <div class="card mb-4" style="padding:0;">
          <div style="padding:16px 20px 8px;">
            <h3 style="font-size:0.875rem; font-weight:600;">
              Visible packages
            </h3>
            <p class="settings-copy">
              {formatVisiblePackageSummary(packages.length)}
            </p>
          </div>

          {#if packageError}
            <div class="alert alert-error" style="margin:0 20px 20px;">
              {packageError}
            </div>
          {:else if packages.length === 0}
            <div class="empty-state" style="margin:0 20px 20px;">
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
                  <span
                    class="text-muted"
                    style="font-size:0.8125rem; margin-left:8px;"
                  >
                    {ecosystemLabel(pkg.ecosystem)}
                  </span>
                  {#if pkg.visibility}
                    <span class="badge badge-ecosystem" style="margin-left:8px;"
                      >{formatRepositoryVisibilityLabel(pkg.visibility)}</span
                    >
                  {/if}
                  {#if pkg.description}
                    <div class="settings-copy" style="margin-top:6px;">
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
      </div>

      <div class="pkg-detail__sidebar">
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
