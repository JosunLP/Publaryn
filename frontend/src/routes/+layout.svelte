<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import { onMount } from 'svelte';

  import { logout } from '../api/auth';
  import { onUnauthorized } from '../api/client';
  import { authToken, clearSession, syncAuthToken } from '../lib/session';
  import { initializeTheme, themeMode, toggleThemeMode } from '../lib/theme';
  import '../styles/main.css';

  let headerQuery = '';

  onMount(() => {
    initializeTheme();
    syncAuthToken();

    onUnauthorized(() => {
      clearSession();
      void goto('/login', { replaceState: true });
    });

    return () => {
      onUnauthorized(null);
    };
  });

  async function handleHeaderSearchSubmit(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    const query = headerQuery.trim();
    if (!query) {
      await goto('/search');
      return;
    }

    const params = new URLSearchParams({ q: query });
    await goto(`/search?${params.toString()}`);
  }

  async function handleLogout(): Promise<void> {
    try {
      await logout();
    } finally {
      clearSession();
      await goto('/login', { replaceState: true });
    }
  }
</script>

<svelte:head>
  <link
    rel="icon"
    href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect width='32' height='32' rx='6' fill='%232563eb'/%3E%3Ctext x='5' y='23' font-family='monospace' font-weight='bold' font-size='18' fill='white'%3EP%3C/text%3E%3C/svg%3E"
  />
</svelte:head>

<header
  class="site-header border-b border-slate-200/80 bg-white/90 backdrop-blur supports-[backdrop-filter]:bg-white/80 dark:border-slate-800/80 dark:bg-slate-950/85"
>
  <div class="container">
    <a href="/" class="logo" data-sveltekit-preload-data="hover">
      <svg viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg"
        ><rect width="32" height="32" rx="6" fill="#2563eb" /><text
          x="5"
          y="23"
          font-family="monospace"
          font-weight="bold"
          font-size="18"
          fill="white">P</text
        ></svg
      >
      <span>Publaryn</span>
      <span class="badge badge-verified">SvelteKit</span>
    </a>

    <div class="search-bar">
      <form on:submit={handleHeaderSearchSubmit}>
        <input
          bind:value={headerQuery}
          type="search"
          name="q"
          class="search-input"
          placeholder="Search packages…"
          aria-label="Search packages"
          autocomplete="off"
        />
      </form>
    </div>

    <nav class="flex flex-wrap justify-end">
      <a
        href="/search"
        class="btn btn-secondary btn-sm"
        data-sveltekit-preload-data="hover"
        aria-current={$page.url.pathname === '/search' ? 'page' : undefined}
      >
        Search
      </a>
      <button
        class="btn btn-secondary btn-sm"
        type="button"
        aria-pressed={$themeMode === 'dark'}
        on:click={toggleThemeMode}
      >
        {$themeMode === 'dark' ? 'Light mode' : 'Dark mode'}
      </button>
      {#if $authToken}
        <a
          href="/settings"
          class="btn btn-secondary btn-sm"
          data-sveltekit-preload-data="hover"
          aria-current={$page.url.pathname === '/settings' ? 'page' : undefined}
        >
          Settings
        </a>
        <button
          class="btn btn-secondary btn-sm"
          type="button"
          on:click={handleLogout}
        >
          Logout
        </button>
      {:else}
        <a
          href="/login"
          class="btn btn-secondary btn-sm"
          data-sveltekit-preload-data="hover">Sign in</a
        >
        <a
          href="/register"
          class="btn btn-primary btn-sm"
          data-sveltekit-preload-data="hover">Sign up</a
        >
      {/if}
    </nav>
  </div>
</header>

<main class="container">
  <slot />
</main>

<footer class="site-footer">
  <div class="container">
    Publaryn — Secure multi-ecosystem package registry
  </div>
</footer>
