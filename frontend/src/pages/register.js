import { register } from '../api/auth.js';
import { navigate } from '../router.js';
import { escapeHtml } from '../utils/format.js';

export function registerPage({ params, query }, container) {
  render(container);
}

function render(container, error = null) {
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

  const form = container.querySelector('#register-form');
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    const username = form.querySelector('#reg-username').value.trim();
    const email = form.querySelector('#reg-email').value.trim();
    const password = form.querySelector('#reg-password').value;

    if (!username || !email || !password) {
      render(container, 'All fields are required.');
      return;
    }
    if (password.length < 12) {
      render(container, 'Password must be at least 12 characters.');
      return;
    }

    const btn = form.querySelector('button[type="submit"]');
    btn.disabled = true;
    btn.textContent = 'Creating account…';

    try {
      await register({ username, email, password });
      navigate('/login', { replace: true });
    } catch (err) {
      const msg =
        err.status === 409
          ? 'Username or email is already taken.'
          : err.body?.error || err.message || 'Registration failed.';
      render(container, msg);
    }
  });
}
