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

<div class="auth-shell">
  <section class="auth-hero">
    <span class="auth-hero__eyebrow">
      <span class="page-hero__eyebrow-dot" aria-hidden="true"></span>
      Get started
    </span>
    <h1 class="auth-hero__title">Create one account for every package workflow.</h1>
    <p class="auth-hero__copy">
      Register once to publish releases, manage organizations, create tokens,
      configure MFA, and work across every supported ecosystem.
    </p>
    <div class="auth-benefits">
      <div class="auth-benefit">
        <div class="auth-benefit__title">Enterprise-ready identity</div>
        <p class="auth-benefit__copy">
          Strong auth, MFA, and scope-based access built into every write surface.
        </p>
      </div>
      <div class="auth-benefit">
        <div class="auth-benefit__title">One control plane</div>
        <p class="auth-benefit__copy">
          Packages, repositories, organizations, releases, and trusted publishing in one UI.
        </p>
      </div>
    </div>
  </section>

  <section class="auth-card">
    <div class="auth-card__header">
      <h2 class="auth-card__title">Create your account</h2>
      <p class="auth-card__copy">
        Your password must be at least 12 characters.
      </p>
    </div>

    {#if error}
      <div class="alert alert-error">{error}</div>
    {/if}

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
        <div class="form-error text-muted" id="password-hint">
          Minimum 12 characters.
        </div>
      </div>
      <div class="auth-form-actions">
        <button
          type="submit"
          class="btn btn-primary"
          disabled={submitting}
        >
          {submitting ? 'Creating account…' : 'Create account'}
        </button>
      </div>
    </form>
    <p class="auth-card__footer">
      Already have an account?
      <a href="/login" data-sveltekit-preload-data="hover">Sign in</a>
    </p>
  </section>
</div>
