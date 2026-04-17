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

<div class="mt-6" style="max-width:400px; margin-left:auto; margin-right:auto;">
  <h1 style="text-align:center; margin-bottom:24px;">Sign in</h1>
  {#if error}
    <div class="alert alert-error">{error}</div>
  {/if}

  <div class="card">
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
        <button
          type="submit"
          class="btn btn-primary"
          style="width:100%; justify-content:center;"
          disabled={mfaSubmitting}
        >
          {mfaSubmitting ? 'Verifying…' : 'Complete sign in'}
        </button>
      </form>
    {:else}
      <form id="login-form" on:submit={handleLoginSubmit}>
        <div class="form-group">
          <label for="login-username">Username</label>
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
        <button
          type="submit"
          class="btn btn-primary"
          style="width:100%; justify-content:center;"
          disabled={submitting}
        >
          {submitting ? 'Signing in…' : 'Sign in'}
        </button>
      </form>
      <p style="text-align:center; margin-top:16px; font-size:0.875rem;">
        Don't have an account? <a
          href="/register"
          data-sveltekit-preload-data="hover">Sign up</a
        >
      </p>
    {/if}
  </div>
</div>
