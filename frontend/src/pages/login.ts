import { login } from '../api/auth';
import { ApiError } from '../api/client';
import type { RouteContext } from '../router';
import { navigate } from '../router';
import { escapeHtml } from '../utils/format';

export function loginPage(_ctx: RouteContext, container: HTMLElement): void {
  render(container);
}

function render(container: HTMLElement, error: string | null = null): void {
  container.innerHTML = `
    <div class="mt-6" style="max-width:400px; margin-left:auto; margin-right:auto;">
      <h1 style="text-align:center; margin-bottom:24px;">Sign in</h1>
      ${error ? `<div class="alert alert-error">${escapeHtml(error)}</div>` : ''}
      <div class="card">
        <form id="login-form">
          <div class="form-group">
            <label for="login-username">Username</label>
            <input
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
              type="password"
              id="login-password"
              name="password"
              class="form-input"
              required
              autocomplete="current-password"
            />
          </div>
          <button type="submit" class="btn btn-primary" style="width:100%; justify-content:center;">
            Sign in
          </button>
        </form>
        <p style="text-align:center; margin-top:16px; font-size:0.875rem;">
          Don't have an account? <a href="/register">Sign up</a>
        </p>
      </div>
    </div>
  `;

  const form = container.querySelector<HTMLFormElement>('#login-form');
  if (!form) {
    return;
  }

  form.addEventListener('submit', async (event) => {
    event.preventDefault();

    const usernameInput =
      form.querySelector<HTMLInputElement>('#login-username');
    const passwordInput =
      form.querySelector<HTMLInputElement>('#login-password');
    const username = usernameInput?.value.trim() ?? '';
    const password = passwordInput?.value ?? '';

    if (!username || !password) {
      render(container, 'Username and password are required.');
      return;
    }

    const submitButton = form.querySelector<HTMLButtonElement>(
      'button[type="submit"]'
    );

    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Signing in…';

    try {
      await login({ usernameOrEmail: username, password });
      navigate('/', { replace: true });
    } catch (caughtError: unknown) {
      const message =
        caughtError instanceof ApiError && caughtError.status === 401
          ? 'Invalid username or password.'
          : caughtError instanceof Error && caughtError.message
            ? caughtError.message
            : 'Login failed. Please try again.';

      render(container, message);
    }
  });
}
