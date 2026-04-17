<script lang="ts">
  import { goto } from '$app/navigation';

  import { register } from '../../api/auth';
  import { ApiError } from '../../api/client';

  let username = '';
  let email = '';
  let password = '';
  let error: string | null = null;
  let submitting = false;

  async function handleSubmit(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!username.trim() || !email.trim() || !password) {
      error = 'All fields are required.';
      return;
    }

    if (password.length < 12) {
      error = 'Password must be at least 12 characters.';
      return;
    }

    submitting = true;
    error = null;

    try {
      await register({
        username: username.trim(),
        email: email.trim(),
        password,
      });
      await goto('/login', { replaceState: true });
    } catch (caughtError: unknown) {
      error =
        caughtError instanceof ApiError && caughtError.status === 409
          ? 'Username or email is already taken.'
          : caughtError instanceof ApiError &&
              caughtError.body &&
              typeof caughtError.body === 'object' &&
              'error' in caughtError.body &&
              typeof caughtError.body.error === 'string'
            ? caughtError.body.error
            : caughtError instanceof Error && caughtError.message
              ? caughtError.message
              : 'Registration failed.';
    } finally {
      submitting = false;
    }
  }
</script>

<svelte:head>
  <title>Sign up — Publaryn</title>
</svelte:head>

<div class="mt-6" style="max-width:400px; margin-left:auto; margin-right:auto;">
  <h1 style="text-align:center; margin-bottom:24px;">Create an account</h1>
  {#if error}
    <div class="alert alert-error">{error}</div>
  {/if}

  <div class="card">
    <form id="register-form" on:submit={handleSubmit}>
      <div class="form-group">
        <label for="reg-username">Username</label>
        <input
          bind:value={username}
          type="text"
          id="reg-username"
          name="username"
          class="form-input"
          required
          minlength="3"
          maxlength="39"
          pattern="^[a-zA-Z0-9]([a-zA-Z0-9_-]*[a-zA-Z0-9])?$"
          autocomplete="username"
          autocapitalize="none"
          spellcheck="false"
          title="3–39 characters, letters/digits/hyphens/underscores, must start and end with a letter or digit."
        />
      </div>
      <div class="form-group">
        <label for="reg-email">Email</label>
        <input
          bind:value={email}
          type="email"
          id="reg-email"
          name="email"
          class="form-input"
          required
          autocomplete="email"
        />
      </div>
      <div class="form-group">
        <label for="reg-password">Password</label>
        <input
          bind:value={password}
          type="password"
          id="reg-password"
          name="password"
          class="form-input"
          required
          minlength="12"
          autocomplete="new-password"
        />
        <div
          class="form-error"
          id="password-hint"
          style="color:var(--color-text-muted);"
        >
          Minimum 12 characters.
        </div>
      </div>
      <button
        type="submit"
        class="btn btn-primary"
        style="width:100%; justify-content:center;"
        disabled={submitting}
      >
        {submitting ? 'Creating account…' : 'Create account'}
      </button>
    </form>
    <p style="text-align:center; margin-top:16px; font-size:0.875rem;">
      Already have an account? <a
        href="/login"
        data-sveltekit-preload-data="hover">Sign in</a
      >
    </p>
  </div>
</div>
