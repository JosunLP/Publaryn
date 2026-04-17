<script lang="ts">
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';

  import type { StatsResponse } from '../api/packages';
  import { getStats } from '../api/packages';
  import { ECOSYSTEMS } from '../utils/ecosystem';
  import { formatNumber } from '../utils/format';

  let stats: StatsResponse = { packages: 0, releases: 0, organizations: 0 };
  let query = '';

  onMount(async () => {
    try {
      stats = await getStats();
    } catch {
      // The landing page still works without stats.
    }
  });

  async function handleSearchSubmit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const trimmedQuery = query.trim();
    if (!trimmedQuery) {
      return;
    }

    await goto(`/search?q=${encodeURIComponent(trimmedQuery)}`);
  }
</script>

<svelte:head>
  <title>Publaryn — Secure multi-ecosystem package registry</title>
</svelte:head>

<section class="hero">
  <h1>Publaryn</h1>
  <p>A secure, multi-ecosystem package registry for modern software teams.</p>
  <div class="search-bar">
    <form id="hero-search-form" on:submit={handleSearchSubmit}>
      <input
        bind:value={query}
        type="search"
        name="q"
        class="search-input"
        placeholder="Search packages across all ecosystems…"
        aria-label="Search packages"
        autocomplete="off"
      />
    </form>
  </div>
</section>

<section class="stats-bar">
  <div class="stat">
    <div class="stat__value">{formatNumber(stats.packages)}</div>
    <div class="stat__label">Packages</div>
  </div>
  <div class="stat">
    <div class="stat__value">{formatNumber(stats.releases)}</div>
    <div class="stat__label">Releases</div>
  </div>
  <div class="stat">
    <div class="stat__value">{formatNumber(stats.organizations)}</div>
    <div class="stat__label">Organizations</div>
  </div>
</section>

<section>
  <h2 style="text-align:center; margin-bottom:16px;">Supported Ecosystems</h2>
  <div class="ecosystem-grid">
    {#each ECOSYSTEMS as ecosystem}
      <a
        href={`/search?ecosystem=${ecosystem.id}`}
        class="ecosystem-tile"
        data-sveltekit-preload-data="hover"
      >
        <span style="font-size:1.5rem">{ecosystem.icon}</span>
        <span>{ecosystem.label}</span>
      </a>
    {/each}
  </div>
</section>
