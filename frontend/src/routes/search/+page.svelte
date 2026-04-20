<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { onMount } from 'svelte';

  import { getAuthToken } from '../../api/client';
  import type { OrganizationMembership } from '../../api/orgs';
  import { listMyOrganizations } from '../../api/orgs';
  import type { SearchPackagesResponse } from '../../api/packages';
  import { searchPackages } from '../../api/packages';
  import {
    buildSearchPath,
    getSearchViewFromQuery,
  } from '../../pages/search-query';
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
  let results: SearchPackagesResponse = {
    total: 0,
    packages: [],
    page: 1,
    per_page: PER_PAGE,
  };
  let q = '';
  let ecosystem = '';
  let org = '';

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
  $: currentPage = searchView.page;
  $: loadKey = `${q}|${ecosystem}|${org}|${currentPage}`;
  $: if (loadKey !== lastLoadKey) {
    lastLoadKey = loadKey;
    void loadResults();
  }

  async function loadOrganizations(): Promise<void> {
    organizationLoadError = null;

    try {
      const data = await listMyOrganizations();
      organizations = (data.organizations || []).filter(
        (membership) => Boolean(membership.slug?.trim())
      );
    } catch (caughtError: unknown) {
      organizationLoadError =
        caughtError instanceof Error
          ? caughtError.message
          : 'Failed to load organizations.';
      organizations = [];
    }
  }

  async function loadResults(): Promise<void> {
    loading = true;
    error = null;

    try {
      results = await searchPackages({
        q: q || undefined,
        ecosystem: ecosystem || undefined,
        org: org || undefined,
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

  $: totalPages = Math.max(1, Math.ceil((results.total || 0) / PER_PAGE));
  $: organizationOptions = [...organizations].sort((left, right) =>
    `${left.name || left.slug || ''}`.localeCompare(`${right.name || right.slug || ''}`)
  );
  $: hasOrganizationOptions = organizationOptions.length > 0 || Boolean(org);
</script>

<svelte:head>
  <title>Search — Publaryn</title>
</svelte:head>

<div class="mt-6">
  <form
    id="search-form"
    style="display:flex; gap:12px; margin-bottom:20px; flex-wrap:wrap;"
    on:submit={handleSearchSubmit}
  >
    <input
      bind:value={q}
      type="search"
      name="q"
      class="search-input"
      placeholder="Search packages…"
      aria-label="Search packages"
      style="flex:1; min-width:200px;"
    />
    <select
      bind:value={ecosystem}
      name="ecosystem"
      class="form-input"
      style="width:auto; min-width:140px;"
    >
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
        style="width:auto; min-width:180px;"
      >
        <option value="">All owners</option>
        {#if org && !organizationOptions.some((membership) => membership.slug === org)}
          <option value={org}>{org}</option>
        {/if}
        {#each organizationOptions as membership}
          <option value={membership.slug || ''}>
            {membership.name || membership.slug || 'Unnamed organization'}
          </option>
        {/each}
      </select>
    {/if}
    <button type="submit" class="btn btn-primary">Search</button>
  </form>
  {#if organizationLoadError}
    <div class="text-muted mb-4" style="font-size:0.875rem;">
      Organization filters are unavailable: {organizationLoadError}
    </div>
  {:else if org}
    <div class="text-muted mb-4" style="font-size:0.875rem;">
      Scoped to packages owned by {org}.
    </div>
  {/if}

  {#if error}
    <div class="alert alert-error">Search failed: {error}</div>
  {:else}
    <div class="text-muted mb-4" style="font-size:0.875rem;">
      {results.total} result{results.total === 1 ? '' : 's'} found
    </div>

    <div class="card" style="padding:0;">
      {#if loading}
        <div class="loading"><span class="spinner"></span> Searching…</div>
      {:else if results.packages.length === 0}
        <div class="empty-state">
          <h3>No packages found</h3>
          <p>Try a different search term or browse by ecosystem.</p>
        </div>
      {:else}
        {#each results.packages as pkg}
          <a
            href={`/packages/${encodeURIComponent(pkg.ecosystem || 'unknown')}/${encodeURIComponent(pkg.name)}`}
            class="package-card"
            data-sveltekit-preload-data="hover"
          >
            <div class="package-card__header">
              <span class="package-card__name"
                >{pkg.display_name || pkg.name}</span
              >
              <span class="badge badge-ecosystem"
                >{ecosystemIcon(pkg.ecosystem)}
                {ecosystemLabel(pkg.ecosystem)}</span
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
              {#if pkg.download_count != null}<span
                  >{formatNumber(pkg.download_count)} downloads</span
                >{/if}
              {#if pkg.updated_at}<span
                  >updated {formatDate(pkg.updated_at)}</span
                >{/if}
            </div>
          </a>
        {/each}
      {/if}
    </div>

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
