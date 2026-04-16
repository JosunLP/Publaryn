import { login } from '../api/auth.js';
import { navigate } from '../router.js';
import { escapeHtml } from '../utils/format.js';

export function loginPage({ params, query }, container) {
  render(container);
}

function render(container, error = null) {
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

  const form = container.querySelector('#login-form');
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    const username = form.querySelector('#login-username').value.trim();
    const password = form.querySelector('#login-password').value;

    if (!username || !password) {
      render(container, 'Username and password are required.');
      return;
    }

    const btn = form.querySelector('button[type="submit"]');
    btn.disabled = true;
    btn.textContent = 'Signing in…';

    try {
      await login({ username, password });
      navigate('/', { replace: true });
    } catch (err) {
      const msg =
        err.status === 401
          ? 'Invalid username or password.'
          : err.message || 'Login failed. Please try again.';
      render(container, msg);
    }
  });
}
