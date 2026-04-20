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
  <meta
    name="description"
    content="A secure, multi-ecosystem package registry for modern software teams."
  />
</svelte:head>

<section class="hero">
  <span class="hero__eyebrow">
    <span class="hero__eyebrow-dot" aria-hidden="true"></span>
    One registry. Every ecosystem.
  </span>
  <h1>
    <span class="hero__headline-line">The package registry</span>
    <span class="hero__headline-line">built for serious teams.</span>
  </h1>
  <p>
    Publish, discover, and secure software across npm, PyPI, Cargo, NuGet,
    Maven, RubyGems, Composer, Bun and OCI — from a single, unified platform.
  </p>
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
  <div class="hero__cta">
    <a href="/register" class="btn btn-primary btn-lg">Get started — it's free</a>
    <a href="/search" class="btn btn-secondary btn-lg">Browse packages</a>
  </div>
</section>

<section class="stats-bar" aria-label="Registry statistics">
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
  <h2 class="section-title">Supported ecosystems</h2>
  <p class="section-subtitle">
    Native protocols, authentic client tooling, zero lock-in.
  </p>
  <div class="ecosystem-grid">
    {#each ECOSYSTEMS as ecosystem}
      <a
        href={`/search?ecosystem=${ecosystem.id}`}
        class="ecosystem-tile"
        data-sveltekit-preload-data="hover"
      >
        <span class="ecosystem-tile__icon" aria-hidden="true"
          >{ecosystem.icon}</span
        >
        <span>{ecosystem.label}</span>
      </a>
    {/each}
  </div>
</section>
