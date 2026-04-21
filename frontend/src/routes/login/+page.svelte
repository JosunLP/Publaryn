<script lang="ts">
  import { goto } from '$app/navigation';

  import { completeMfaChallenge, login } from '../../api/auth';
  import { ApiError } from '../../api/client';
  import { syncAuthToken } from '../../lib/session';

  let username = '';
  let password = '';
  let mfaCode = '';
  let error: string | null = null;
  let submitting = false;
  let mfaSubmitting = false;
  let mfaToken: string | null = null;

  async function handleLoginSubmit(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    const trimmedUsername = username.trim();
    if (!trimmedUsername || !password) {
      error = 'Username and password are required.';
      return;
    }

    submitting = true;
    error = null;

    try {
      const result = await login({
        usernameOrEmail: trimmedUsername,
        password,
      });

      if (result.token) {
        syncAuthToken();
        await goto('/', { replaceState: true });
        return;
      }

      if (result.mfa_token) {
        mfaToken = result.mfa_token;
        mfaCode = '';
        return;
      }

      error = 'Sign in did not return a session token.';
    } catch (caughtError: unknown) {
      error =
        caughtError instanceof ApiError && caughtError.status === 401
          ? 'Invalid username or password.'
          : caughtError instanceof Error && caughtError.message
            ? caughtError.message
            : 'Login failed. Please try again.';
    } finally {
      submitting = false;
    }
  }

  async function handleMfaSubmit(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!mfaToken || !mfaCode.trim()) {
      error = 'Enter the verification code from your authenticator app.';
      return;
    }

    mfaSubmitting = true;
    error = null;

    try {
      const result = await completeMfaChallenge({
        mfaToken,
        code: mfaCode.trim(),
      });

      if (!result.token) {
        error =
          'The MFA challenge completed but no session token was returned.';
        return;
      }

      syncAuthToken();
      await goto('/', { replaceState: true });
    } catch (caughtError: unknown) {
      error =
        caughtError instanceof ApiError && caughtError.status === 401
          ? 'The MFA code is invalid or expired.'
          : caughtError instanceof Error && caughtError.message
            ? caughtError.message
            : 'MFA verification failed.';
    } finally {
      mfaSubmitting = false;
    }
  }
</script>

<svelte:head>
  <title>Sign in — Publaryn</title>
</svelte:head>

<div class="auth-shell">
  <section class="auth-hero">
    <span class="auth-hero__eyebrow">
      <span class="page-hero__eyebrow-dot" aria-hidden="true"></span>
      Welcome back
    </span>
    <h1 class="auth-hero__title">A seamless registry experience, end to end.</h1>
    <p class="auth-hero__copy">
      Sign in to manage packages, organizations, releases, security triage, and
      delegated access — all in one place.
    </p>
    <div class="auth-benefits">
      <div class="auth-benefit">
        <div class="auth-benefit__title">Unified package control</div>
        <p class="auth-benefit__copy">
          Manage npm, PyPI, Cargo, NuGet, Maven, Composer, OCI, and more from one workspace.
        </p>
      </div>
      <div class="auth-benefit">
        <div class="auth-benefit__title">Security-first workflows</div>
        <p class="auth-benefit__copy">
          Review findings, enforce trusted publishing, and protect private reads with shared auth.
        </p>
      </div>
    </div>
  </section>

  <section class="auth-card">
    <div class="auth-card__header">
      <h2 class="auth-card__title">{mfaToken ? 'Complete verification' : 'Sign in'}</h2>
      <p class="auth-card__copy">
        {#if mfaToken}
          Enter the code from your authenticator app or a recovery code.
        {:else}
          Use your username or email and password to access Publaryn.
        {/if}
      </p>
    </div>

    {#if error}
      <div class="alert alert-error">{error}</div>
    {/if}

    {#if mfaToken}
      <form id="mfa-form" on:submit={handleMfaSubmit}>
        <div class="form-group">
          <label for="mfa-code">Authenticator or recovery code</label>
          <input
            bind:value={mfaCode}
            type="text"
            id="mfa-code"
            name="code"
            class="form-input"
            required
            autocomplete="one-time-code"
          />
        </div>
        <div class="auth-form-actions">
          <button
            type="submit"
            class="btn btn-primary"
            disabled={mfaSubmitting}
          >
            {mfaSubmitting ? 'Verifying…' : 'Complete sign in'}
          </button>
        </div>
      </form>
    {:else}
      <form id="login-form" on:submit={handleLoginSubmit}>
        <div class="form-group">
          <label for="login-username">Username or email</label>
          <input
            bind:value={username}
            type="text"
            id="login-username"
            name="username"
            class="form-input"
            required
            autocomplete="username"
            autocapitalize="none"
            spellcheck="false"
          />
        </div>
        <div class="form-group">
          <label for="login-password">Password</label>
          <input
            bind:value={password}
            type="password"
            id="login-password"
            name="password"
            class="form-input"
            required
            autocomplete="current-password"
          />
        </div>
        <div class="auth-form-actions">
          <button
            type="submit"
            class="btn btn-primary"
            disabled={submitting}
          >
            {submitting ? 'Signing in…' : 'Sign in'}
          </button>
        </div>
      </form>
      <p class="auth-card__footer">
        Don’t have an account?
        <a href="/register" data-sveltekit-preload-data="hover">Create one</a>
      </p>
    {/if}
  </section>
</div>
