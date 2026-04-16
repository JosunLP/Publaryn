import { register } from '../api/auth';
import { ApiError } from '../api/client';
import type { RouteContext } from '../router';
import { navigate } from '../router';
import { escapeHtml } from '../utils/format';

export function registerPage(_ctx: RouteContext, container: HTMLElement): void {
  render(container);
}

function render(container: HTMLElement, error: string | null = null): void {
  container.innerHTML = `
    <div class="mt-6" style="max-width:400px; margin-left:auto; margin-right:auto;">
      <h1 style="text-align:center; margin-bottom:24px;">Create an account</h1>
      ${error ? `<div class="alert alert-error">${escapeHtml(error)}</div>` : ''}
      <div class="card">
        <form id="register-form">
          <div class="form-group">
            <label for="reg-username">Username</label>
            <input
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
              type="password"
              id="reg-password"
              name="password"
              class="form-input"
              required
              minlength="12"
              autocomplete="new-password"
            />
            <div class="form-error" id="password-hint" style="color:var(--color-text-muted);">
              Minimum 12 characters.
            </div>
          </div>
          <button type="submit" class="btn btn-primary" style="width:100%; justify-content:center;">
            Create account
          </button>
        </form>
        <p style="text-align:center; margin-top:16px; font-size:0.875rem;">
          Already have an account? <a href="/login">Sign in</a>
        </p>
      </div>
    </div>
  `;

  const form = container.querySelector<HTMLFormElement>('#register-form');
  if (!form) {
    return;
  }

  form.addEventListener('submit', async (event) => {
    event.preventDefault();

    const usernameInput = form.querySelector<HTMLInputElement>('#reg-username');
    const emailInput = form.querySelector<HTMLInputElement>('#reg-email');
    const passwordInput = form.querySelector<HTMLInputElement>('#reg-password');

    const username = usernameInput?.value.trim() ?? '';
    const email = emailInput?.value.trim() ?? '';
    const password = passwordInput?.value ?? '';

    if (!username || !email || !password) {
      render(container, 'All fields are required.');
      return;
    }

    if (password.length < 12) {
      render(container, 'Password must be at least 12 characters.');
      return;
    }

    const submitButton = form.querySelector<HTMLButtonElement>(
      'button[type="submit"]'
    );

    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Creating account…';

    try {
      await register({ username, email, password });
      navigate('/login', { replace: true });
    } catch (caughtError: unknown) {
      const message =
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

      render(container, message);
    }
  });
}
