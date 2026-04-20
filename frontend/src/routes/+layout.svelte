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
    href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Cdefs%3E%3ClinearGradient id='g' x1='0' y1='0' x2='1' y2='1'%3E%3Cstop offset='0%25' stop-color='%230a84ff'/%3E%3Cstop offset='100%25' stop-color='%23af52de'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width='32' height='32' rx='8' fill='url(%23g)'/%3E%3Ctext x='6' y='23' font-family='-apple-system,SF Pro Display,Helvetica,Arial' font-weight='700' font-size='18' fill='white'%3EP%3C/text%3E%3C/svg%3E"
  />
</svelte:head>

<a class="skip-link sr-only" href="#main-content">Skip to main content</a>

<header class="site-header">
  <div class="container">
    <a href="/" class="logo" data-sveltekit-preload-data="hover">
      <svg viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg"
        ><defs
          ><linearGradient id="publaryn-logo-grad" x1="0" y1="0" x2="1" y2="1"
            ><stop offset="0%" stop-color="#0a84ff" /><stop
              offset="100%"
              stop-color="#af52de"
            /></linearGradient
          ></defs
        ><rect width="32" height="32" rx="8" fill="url(#publaryn-logo-grad)" /><text
          x="6"
          y="23"
          font-family="-apple-system, SF Pro Display, Helvetica, Arial"
          font-weight="700"
          font-size="18"
          fill="white">P</text
        ></svg
      >
      <span>Publaryn</span>
    </a>

    <div class="search-bar">
      <form on:submit={handleHeaderSearchSubmit}>
        <input
          bind:value={headerQuery}
          type="search"
          name="q"
          class="search-input"
          placeholder="Search packages across every ecosystem…"
          aria-label="Search packages"
          autocomplete="off"
        />
      </form>
    </div>

    <nav aria-label="Primary">
      <a
        href="/search"
        class="btn btn-ghost btn-sm"
        data-sveltekit-preload-data="hover"
        aria-current={$page.url.pathname === '/search' ? 'page' : undefined}
      >
        Explore
      </a>
      <button
        class="btn btn-ghost btn-sm theme-toggle"
        type="button"
        aria-label={$themeMode === 'dark'
          ? 'Switch to light mode'
          : 'Switch to dark mode'}
        aria-pressed={$themeMode === 'dark'}
        title={$themeMode === 'dark'
          ? 'Switch to light mode'
          : 'Switch to dark mode'}
        on:click={toggleThemeMode}
      >
        {#if $themeMode === 'dark'}
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            ><circle cx="12" cy="12" r="4" /><path
              d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41"
            /></svg
          >
        {:else}
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            ><path
              d="M21 12.79A9 9 0 1 1 11.21 3a7 7 0 0 0 9.79 9.79Z"
            /></svg
          >
        {/if}
      </button>
      {#if $authToken}
        <a
          href="/settings"
          class="btn btn-ghost btn-sm"
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
          Sign out
        </button>
      {:else}
        <a
          href="/login"
          class="btn btn-ghost btn-sm"
          data-sveltekit-preload-data="hover">Sign in</a
        >
        <a
          href="/register"
          class="btn btn-primary btn-sm"
          data-sveltekit-preload-data="hover">Get started</a
        >
      {/if}
    </nav>
  </div>
</header>

<main id="main-content" class="container">
  <slot />
</main>

<footer class="site-footer">
  <div class="container">
    <span>Publaryn — Secure multi-ecosystem package registry</span>
  </div>
</footer>

<style>
  .skip-link {
    position: absolute;
    top: 8px;
    left: 8px;
    z-index: 200;
    padding: 8px 14px;
    background: var(--color-primary);
    color: #fff;
    border-radius: var(--radius-md);
  }
  .skip-link:focus {
    width: auto;
    height: auto;
    clip: auto;
    margin: 0;
    overflow: visible;
    white-space: normal;
  }
  .theme-toggle {
    width: 36px;
    height: 32px;
    padding: 0;
  }
</style>

