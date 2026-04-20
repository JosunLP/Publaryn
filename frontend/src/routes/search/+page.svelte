<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { onMount } from 'svelte';

  import { getAuthToken } from '../../api/client';
  import type {
    OrganizationMembership,
    OrgRepositorySummary,
  } from '../../api/orgs';
  import { listMyOrganizations, listOrgRepositories } from '../../api/orgs';
  import type { SearchPackagesResponse } from '../../api/packages';
  import { searchPackages } from '../../api/packages';
  import {
    buildSearchPath,
    getSearchViewFromQuery,
  } from '../../pages/search-query';
  import { formatSearchResultRepository } from '../../pages/search-results';
  import {
    ECOSYSTEMS,
    ecosystemIcon,
    ecosystemLabel,
    formatVersionLabel,
  } from '../../utils/ecosystem';
  import { formatDate, formatNumber } from '../../utils/format';
  import { formatRepositoryVisibilityLabel } from '../../utils/repositories';

  const PER_PAGE = 20;

  let lastLoadKey = '';
  let loading = true;
  let error: string | null = null;
  let organizationLoadError: string | null = null;
  let organizations: OrganizationMembership[] = [];
  let repositoryLoadError: string | null = null;
  let repositories: OrgRepositorySummary[] = [];
  let lastLoadedRepositoryOrg = '';
  let currentOrgInOptions = false;
  let hasOrganizationOptions = false;
  let currentRepositoryInOptions = false;
  let hasRepositoryOptions = false;
  let results: SearchPackagesResponse = {
    total: 0,
    packages: [],
    page: 1,
    per_page: PER_PAGE,
  };
  let q = '';
  let ecosystem = '';
  let org = '';
  let repository = '';

  onMount(() => {
    if (!getAuthToken()) {
      organizations = [];
      organizationLoadError = null;
      return;
    }

    void loadOrganizations();
  });

  $: searchView = getSearchViewFromQuery($page.url.searchParams);
  $: q = searchView.q;
  $: ecosystem = searchView.ecosystem;
  $: org = searchView.org;
  $: repository = searchView.repository;
  $: currentPage = searchView.page;
  $: loadKey = `${q}|${ecosystem}|${org}|${repository}|${currentPage}`;
  $: if (loadKey !== lastLoadKey) {
    lastLoadKey = loadKey;
    void loadResults();
  }
  $: if (org !== lastLoadedRepositoryOrg) {
    lastLoadedRepositoryOrg = org;
    if (!org) {
      repositories = [];
      repositoryLoadError = null;
    } else {
      void loadRepositories(org);
    }
  }

  async function loadOrganizations(): Promise<void> {
    organizationLoadError = null;

    try {
      const data = await listMyOrganizations();
      organizations = [...(data.organizations || [])]
        .filter((membership) => Boolean(membership.slug?.trim()))
        .sort((left, right) =>
          `${left.name || left.slug || ''}`.localeCompare(`${right.name || right.slug || ''}`)
        );
    } catch (caughtError: unknown) {
      organizationLoadError =
        caughtError instanceof Error
          ? caughtError.message
          : 'Failed to load organizations.';
      organizations = [];
    }
  }

  async function loadRepositories(orgSlug: string): Promise<void> {
    repositoryLoadError = null;
    let data: Awaited<ReturnType<typeof listOrgRepositories>>;

    try {
      data = await listOrgRepositories(orgSlug);
    } catch (caughtError: unknown) {
      if (org !== orgSlug) {
        return;
      }

      repositoryLoadError =
        caughtError instanceof Error
          ? caughtError.message
          : 'Failed to load repositories.';
      repositories = [];
      return;
    }

    if (org !== orgSlug) {
      return;
    }

    repositories = [...(data.repositories || [])]
      .filter((repositoryOption) => Boolean(repositoryOption.slug?.trim()))
      .sort((left, right) =>
        `${left.name || left.slug || ''}`.localeCompare(
          `${right.name || right.slug || ''}`
        )
      );
  }

  async function loadResults(): Promise<void> {
    loading = true;
    error = null;

    try {
      results = await searchPackages({
        q: q || undefined,
        ecosystem: ecosystem || undefined,
        org: org || undefined,
        repository: repository || undefined,
        page: currentPage,
        perPage: PER_PAGE,
      });
    } catch (caughtError: unknown) {
      error =
        caughtError instanceof Error ? caughtError.message : 'Search failed.';
      results = {
        total: 0,
        packages: [],
        page: 1,
        per_page: PER_PAGE,
      };
    } finally {
      loading = false;
    }
  }

  async function handleSearchSubmit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    await goto(
      buildSearchPath(
        {
          q,
          ecosystem,
          org,
          repository,
          page: 1,
        },
        $page.url.searchParams
      )
    );
  }

  async function goToPage(nextPage: number): Promise<void> {
    await goto(
      buildSearchPath(
        {
          q,
          ecosystem,
          org,
          repository,
          page: nextPage,
        },
        $page.url.searchParams
      )
    );
  }

  function shouldShowVisibilityBadge(
    visibility: string | null | undefined
  ): boolean {
    return Boolean(visibility && visibility !== 'public');
  }

  function repositoryLabel(
    repositoryName: string | null | undefined,
    repositorySlug: string | null | undefined
  ): string {
    return formatSearchResultRepository({
      repository_name: repositoryName,
      repository_slug: repositorySlug,
    });
  }

  $: totalPages = Math.max(1, Math.ceil((results.total || 0) / PER_PAGE));
  $: currentOrgInOptions = organizations.some((membership) => membership.slug === org);
  $: hasOrganizationOptions = organizations.length > 0 || Boolean(org);
  $: currentRepositoryInOptions = repositories.some(
    (repositoryOption) => repositoryOption.slug === repository
  );
  $: hasRepositoryOptions = repositories.length > 0 || Boolean(repository) || Boolean(org);
</script>

<svelte:head>
  <title>Search — Publaryn</title>
</svelte:head>

<div class="page-shell">
  <section class="page-hero">
    <div class="page-hero__header">
      <div class="page-hero__copy">
        <span class="page-hero__eyebrow">
          <span class="page-hero__eyebrow-dot" aria-hidden="true"></span>
          Discover
        </span>
        <h1 class="page-hero__title">Search every package surface</h1>
        <p class="page-hero__subtitle">
          Explore packages across ecosystems, then narrow results by organization
          and repository when you have access.
        </p>
        <div class="page-hero__meta">
          {#if results.total > 0}
            <span class="badge badge-ecosystem"
              >{formatNumber(results.total)} results</span
            >
          {/if}
          {#if ecosystem}
            <span class="badge badge-ecosystem">{ecosystemLabel(ecosystem)}</span>
          {/if}
          {#if org}
            <span class="badge badge-ecosystem">Owner · {org}</span>
          {/if}
          {#if repository}
            <span class="badge badge-ecosystem">Repository · {repository}</span>
          {/if}
        </div>
      </div>
    </div>
  </section>

  <form id="search-form" class="toolbar" on:submit={handleSearchSubmit}>
    <div class="filter-grid filter-grid--search">
      <input
        bind:value={q}
        type="search"
        name="q"
        class="search-input"
        placeholder="Search packages, owners, and repositories…"
        aria-label="Search packages"
      />
      <select bind:value={ecosystem} name="ecosystem" class="form-input">
        <option value="">All ecosystems</option>
        {#each ECOSYSTEMS as option}
          <option value={option.id}>{option.label}</option>
        {/each}
      </select>
      {#if hasOrganizationOptions}
        <select
          bind:value={org}
          name="org"
          class="form-input"
          aria-label="Organization scope"
          on:change={() => (repository = '')}
        >
          <option value="">All owners</option>
          {#if org && !currentOrgInOptions}
            <option value={org}>{org}</option>
          {/if}
          {#each organizations as membership}
            <option value={membership.slug || ''}>
              {membership.name || membership.slug || 'Unnamed organization'}
            </option>
          {/each}
        </select>
      {/if}
      {#if hasRepositoryOptions}
        <select
          bind:value={repository}
          name="repository"
          class="form-input"
          aria-label="Repository scope"
        >
          <option value="">All repositories</option>
          {#if repository && !currentRepositoryInOptions}
            <option value={repository}>{repository}</option>
          {/if}
          {#each repositories as repositoryOption}
            <option value={repositoryOption.slug || ''}>
              {repositoryOption.name || repositoryOption.slug || 'Unnamed repository'}
            </option>
          {/each}
        </select>
      {/if}
    </div>
    <div class="page-hero__actions">
      <button type="submit" class="btn btn-primary">Search</button>
    </div>
  </form>

  {#if organizationLoadError}
    <div class="notice notice--warning">
      Organization filters are unavailable: {organizationLoadError}
    </div>
  {:else if org}
    <div class="toolbar__meta">Scoped to packages owned by {org}.</div>
  {/if}

  {#if repositoryLoadError}
    <div class="notice notice--warning">
      Repository filters are unavailable: {repositoryLoadError}
    </div>
  {:else if repository}
    <div class="toolbar__meta">Scoped to repository {repository}.</div>
  {/if}

  {#if error}
    <div class="alert alert-error">Search failed: {error}</div>
  {:else}
    <section class="surface-card">
      <div class="surface-card__header">
        <div class="surface-card__title">
          {results.total} result{results.total === 1 ? '' : 's'} found
        </div>
        <p class="surface-card__copy">
          Native package metadata, repository ownership, and visibility-aware search.
        </p>
      </div>

      <div class="search-result-shell">
        {#if loading}
          <div class="loading"><span class="spinner"></span> Searching…</div>
        {:else if results.packages.length === 0}
          <div class="empty-state">
            <h3>No packages found</h3>
            <p>Try a different query, clear a filter, or browse by ecosystem.</p>
            <div class="empty-actions">
              <a href="/search" class="btn btn-primary">Clear filters</a>
              <a href="/" class="btn btn-secondary" data-sveltekit-preload-data="hover"
                >Back home</a
              >
            </div>
          </div>
        {:else}
          {#each results.packages as pkg}
            <a
              href={`/packages/${encodeURIComponent(pkg.ecosystem || 'unknown')}/${encodeURIComponent(pkg.name)}`}
              class="package-card"
              data-sveltekit-preload-data="hover"
            >
              <div class="package-card__header">
                <span class="package-card__name">{pkg.display_name || pkg.name}</span>
                <span class="badge badge-ecosystem"
                  >{ecosystemIcon(pkg.ecosystem)} {ecosystemLabel(pkg.ecosystem)}</span
                >
                {#if pkg.latest_version}
                  <span class="package-card__version"
                    >{formatVersionLabel(pkg.ecosystem, pkg.latest_version)}</span
                  >
                {/if}
                {#if pkg.is_deprecated}
                  <span class="badge badge-deprecated">deprecated</span>
                {/if}
                {#if shouldShowVisibilityBadge(pkg.visibility)}
                  <span class="badge"
                    >{formatRepositoryVisibilityLabel(pkg.visibility)}</span
                  >
                {/if}
              </div>
              <div class="package-card__description">{pkg.description || ''}</div>
              <div class="package-card__meta">
                {#if pkg.owner_name}<span>by {pkg.owner_name}</span>{/if}
                {#if repositoryLabel(pkg.repository_name, pkg.repository_slug)}<span
                    >in {repositoryLabel(pkg.repository_name, pkg.repository_slug)}</span
                  >{/if}
                {#if pkg.download_count != null}<span
                    >{formatNumber(pkg.download_count)} downloads</span
                  >{/if}
                {#if pkg.updated_at}<span>updated {formatDate(pkg.updated_at)}</span>{/if}
              </div>
            </a>
          {/each}
        {/if}
      </div>
    </section>

    {#if !loading && totalPages > 1}
      <div class="pagination">
        {#if currentPage > 1}
          <button
            class="btn btn-secondary btn-sm"
            type="button"
            on:click={() => goToPage(currentPage - 1)}>← Prev</button
          >
        {/if}
        <span class="current">Page {currentPage} of {totalPages}</span>
        {#if currentPage < totalPages}
          <button
            class="btn btn-secondary btn-sm"
            type="button"
            on:click={() => goToPage(currentPage + 1)}>Next →</button
          >
        {/if}
      </div>
    {/if}
  {/if}
</div>
