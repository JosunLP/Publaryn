import type { MfaSetupState, UserProfile } from '../api/auth';
import {
  disableMfa,
  getCurrentUser,
  setupMfa,
  updateCurrentUser,
  verifyMfaSetup,
} from '../api/auth';
import { getAuthToken } from '../api/client';
import type {
  MyInvitation,
  MyInvitationListResponse,
  OrganizationListResponse,
  OrganizationMembership,
} from '../api/orgs';
import {
  acceptInvitation,
  createOrg,
  declineInvitation,
  listMyInvitations,
  listMyOrganizations,
} from '../api/orgs';
import type { TokenListResponse, TokenRecord } from '../api/tokens';
import { createToken, listTokens, revokeToken } from '../api/tokens';
import type { RouteContext } from '../router';
import { navigate } from '../router';
import { copyToClipboard, escapeHtml, formatDate } from '../utils/format';

const TOKEN_SCOPE_OPTIONS = [
  'profile:write',
  'tokens:read',
  'tokens:write',
  'orgs:write',
  'orgs:join',
  'orgs:transfer',
  'namespaces:write',
  'repositories:write',
  'packages:write',
  'packages:transfer',
  'audit:read',
] as const;

interface SettingsViewState {
  user: UserProfile;
  tokens: TokenRecord[];
  organizations: OrganizationMembership[];
  organizationsError: string | null;
  invitations: MyInvitation[];
  invitationsError: string | null;
  notice: string | null;
  error: string | null;
  createdToken: string | null;
  mfaSetupState: MfaSetupState | null;
}

export function settingsPage(_ctx: RouteContext, container: HTMLElement): void {
  if (!getAuthToken()) {
    navigate('/login', { replace: true });
    return;
  }

  container.innerHTML = `<div class="loading"><span class="spinner"></span> Loading settings…</div>`;
  void loadAndRender(container);
}

async function loadAndRender(
  container: HTMLElement,
  {
    notice = null,
    error = null,
    createdToken = null,
    mfaSetupState = null,
  }: {
    notice?: string | null;
    error?: string | null;
    createdToken?: string | null;
    mfaSetupState?: MfaSetupState | null;
  } = {}
): Promise<void> {
  try {
    const [user, tokenData, organizationData, invitationData] =
      await Promise.all([
        getCurrentUser(),
        listTokens(),
        listMyOrganizations().catch(
          (caughtError: unknown): OrganizationListResponse => ({
            organizations: [],
            load_error: toErrorMessage(
              caughtError,
              'Failed to load organizations.'
            ),
          })
        ),
        listMyInvitations().catch(
          (caughtError: unknown): MyInvitationListResponse => ({
            invitations: [],
            load_error: toErrorMessage(
              caughtError,
              'Failed to load invitations.'
            ),
          })
        ),
      ]);

    render(container, {
      user,
      tokens: tokenData.tokens || [],
      organizations: organizationData.organizations || [],
      organizationsError: organizationData.load_error || null,
      invitations: invitationData.invitations || [],
      invitationsError: invitationData.load_error || null,
      notice,
      error,
      createdToken,
      mfaSetupState,
    });
  } catch (caughtError: unknown) {
    container.innerHTML = `
      <div class="mt-6">
        <div class="alert alert-error">${escapeHtml(
          toErrorMessage(caughtError, 'Failed to load settings.')
        )}</div>
      </div>
    `;
  }
}

function render(container: HTMLElement, state: SettingsViewState): void {
  const {
    user,
    tokens,
    organizations,
    organizationsError,
    invitations,
    invitationsError,
    notice,
    error,
    createdToken,
    mfaSetupState,
  } = state;

  container.innerHTML = `
    <div class="mt-6 settings-page">
      <div class="settings-header">
        <div>
          <h1>Settings</h1>
          <p class="text-muted">Manage your profile, API tokens, and multi-factor authentication.</p>
        </div>
      </div>

      ${notice ? `<div class="alert alert-success">${escapeHtml(notice)}</div>` : ''}
      ${error ? `<div class="alert alert-error">${escapeHtml(error)}</div>` : ''}

      <div class="settings-grid">
        <section class="card settings-section">
          <h2>Profile</h2>
          <p class="text-muted settings-copy">Your public profile information and account details.</p>

          <form id="profile-form">
            <div class="form-group">
              <label for="settings-username">Username</label>
              <input id="settings-username" class="form-input" value="${escapeHtml(user.username || '')}" disabled />
            </div>
            <div class="form-group">
              <label for="settings-email">Email</label>
              <input id="settings-email" class="form-input" value="${escapeHtml(user.email || '')}" disabled />
            </div>
            <div class="form-group">
              <label for="settings-display-name">Display name</label>
              <input id="settings-display-name" name="display_name" class="form-input" value="${escapeHtml(user.display_name || '')}" />
            </div>
            <div class="form-group">
              <label for="settings-avatar-url">Avatar URL</label>
              <input id="settings-avatar-url" name="avatar_url" class="form-input" value="${escapeHtml(user.avatar_url || '')}" />
            </div>
            <div class="form-group">
              <label for="settings-website">Website</label>
              <input id="settings-website" name="website" class="form-input" value="${escapeHtml(user.website || '')}" />
            </div>
            <div class="form-group">
              <label for="settings-bio">Bio</label>
              <textarea id="settings-bio" name="bio" class="form-input" rows="4">${escapeHtml(user.bio || '')}</textarea>
            </div>
            <button type="submit" class="btn btn-primary">Save profile</button>
          </form>
        </section>

        <section class="card settings-section">
          <h2>Multi-factor authentication</h2>
          <p class="text-muted settings-copy">
            Status: <strong>${user.mfa_enabled ? 'Enabled' : 'Disabled'}</strong>
          </p>

          ${
            user.mfa_enabled
              ? `
                <form id="mfa-disable-form">
                  <div class="form-group">
                    <label for="mfa-disable-code">Authenticator or recovery code</label>
                    <input id="mfa-disable-code" name="code" class="form-input" placeholder="123456 or xxxx-yyyy" required />
                  </div>
                  <button type="submit" class="btn btn-danger">Disable MFA</button>
                </form>
              `
              : `
                <button id="mfa-setup-btn" class="btn btn-primary">Start MFA setup</button>
                <p class="text-muted mt-4">Use an authenticator app like 1Password, Bitwarden, Authy, or Google Authenticator.</p>
              `
          }

          ${
            mfaSetupState
              ? `
                <div class="settings-subsection">
                  <h3>Step 1: Add the secret to your authenticator app</h3>
                  <div class="code-block"><button class="copy-btn" data-copy="mfa-secret">Copy</button><code id="mfa-secret">${escapeHtml(mfaSetupState.secret)}</code></div>
                  <div class="mt-4">
                    <label class="text-muted" style="display:block; margin-bottom:6px;">Provisioning URI</label>
                    <div class="code-block"><button class="copy-btn" data-copy="mfa-uri">Copy</button><code id="mfa-uri">${escapeHtml(mfaSetupState.provisioning_uri)}</code></div>
                  </div>
                  <div class="mt-4">
                    <label class="text-muted" style="display:block; margin-bottom:6px;">Recovery codes (store these somewhere safe)</label>
                    <div class="code-block"><button class="copy-btn" data-copy="mfa-recovery">Copy</button><code id="mfa-recovery">${escapeHtml(mfaSetupState.recovery_codes.join('\n'))}</code></div>
                  </div>
                  <form id="mfa-verify-form" class="mt-4">
                    <div class="form-group">
                      <label for="mfa-verify-code">Step 2: Enter a code from your authenticator app</label>
                      <input id="mfa-verify-code" name="code" class="form-input" placeholder="123456" required />
                    </div>
                    <button type="submit" class="btn btn-primary">Enable MFA</button>
                  </form>
                </div>
              `
              : ''
          }
        </section>
      </div>

      <div class="settings-grid mt-6">
        <section class="card settings-section">
          <h2>Your organizations</h2>
          <p class="text-muted settings-copy">
            Organizations you belong to and the role you currently hold in each one.
          </p>

          ${
            organizationsError
              ? `<div class="alert alert-error">${escapeHtml(organizationsError)}</div>`
              : organizations.length === 0
                ? `<div class="empty-state"><h3>No organizations yet</h3><p>Create one below or accept an invitation to start collaborating.</p></div>`
                : `<div class="token-list">
                    ${organizations
                      .map(
                        (organization) => `
                          <div class="token-row">
                            <div class="token-row__main">
                              <div class="token-row__title">
                                ${
                                  organization.slug
                                    ? `<a href="/orgs/${encodeURIComponent(organization.slug)}">${escapeHtml(organization.name || organization.slug || 'Organization')}</a>`
                                    : escapeHtml(
                                        organization.name ||
                                          organization.slug ||
                                          'Organization'
                                      )
                                }
                                ${organization.is_verified ? '<span class="badge badge-verified">Verified</span>' : ''}
                              </div>
                              <div class="token-row__meta">
                                <span>@${escapeHtml(organization.slug || 'unknown')}</span>
                                <span>role ${escapeHtml(organization.role || 'member')}</span>
                                <span>joined ${escapeHtml(formatDate(organization.joined_at))}</span>
                              </div>
                              <div class="token-row__scopes">
                                <span class="badge badge-ecosystem">${escapeHtml(String(organization.package_count ?? 0))} packages</span>
                                <span class="badge badge-ecosystem">${escapeHtml(String(organization.team_count ?? 0))} teams</span>
                              </div>
                              ${organization.description ? `<p class="settings-copy">${escapeHtml(organization.description)}</p>` : ''}
                            </div>
                            ${
                              organization.slug
                                ? `
                                  <div class="token-row__actions">
                                    <a class="btn btn-secondary btn-sm" href="/orgs/${encodeURIComponent(organization.slug)}">Open workspace</a>
                                  </div>
                                `
                                : ''
                            }
                          </div>
                        `
                      )
                      .join('')}
                  </div>`
          }
        </section>

        <section class="card settings-section">
          <h2>Organization invitations</h2>
          <p class="text-muted settings-copy">
            Accept or decline invitations to join organizations.
          </p>

          ${
            invitationsError
              ? `<div class="alert alert-error">${escapeHtml(invitationsError)}</div>`
              : invitations.length === 0
                ? `<div class="empty-state"><h3>No pending invitations</h3><p>When an organization invites your account, it will appear here.</p></div>`
                : `<div class="token-list">
                    ${invitations
                      .map(
                        (invitation) => `
                          <div class="token-row">
                            <div class="token-row__main">
                              <div class="token-row__title">${escapeHtml(invitation.org?.name || invitation.org?.slug || 'Organization')}</div>
                              <div class="token-row__meta">
                                <span>role ${escapeHtml(invitation.role || 'viewer')}</span>
                                <span>invited by @${escapeHtml(invitation.invited_by?.username || 'unknown')}</span>
                                <span>sent ${escapeHtml(formatDate(invitation.created_at))}</span>
                                <span>${invitation.expires_at ? `expires ${escapeHtml(formatDate(invitation.expires_at))}` : 'no expiry'}</span>
                              </div>
                              <div class="token-row__scopes">
                                <span class="badge badge-verified">${escapeHtml(invitation.status || 'pending')}</span>
                              </div>
                            </div>
                            ${
                              invitation.actionable === false
                                ? ''
                                : `
                                  <div class="token-row__actions">
                                    <button class="btn btn-primary btn-sm" data-accept-invitation="${escapeHtml(invitation.id || '')}" type="button">Accept</button>
                                    <button class="btn btn-secondary btn-sm" data-decline-invitation="${escapeHtml(invitation.id || '')}" type="button">Decline</button>
                                  </div>
                                `
                            }
                          </div>
                        `
                      )
                      .join('')}
                  </div>`
          }
        </section>

        <section class="card settings-section">
          <h2>Create organization</h2>
          <p class="text-muted settings-copy">
            Start a shared workspace for teams, invitations, and delegated package governance.
          </p>

          <form id="org-create-form">
            <div class="form-group">
              <label for="org-name">Organization name</label>
              <input id="org-name" name="name" class="form-input" placeholder="Acme" required />
            </div>

            <div class="form-group">
              <label for="org-slug">Slug</label>
              <input
                id="org-slug"
                name="slug"
                class="form-input"
                placeholder="acme"
                pattern="[a-z0-9][a-z0-9-]{0,63}"
                required
              />
              <div class="text-muted mt-4">
                Lowercase letters, numbers, and hyphens only. Must start with a letter or number.
              </div>
            </div>

            <div class="form-group">
              <label for="org-description">Description</label>
              <textarea id="org-description" name="description" class="form-input" rows="3" placeholder="What this organization publishes and maintains"></textarea>
            </div>

            <div class="form-group">
              <label for="org-website">Website</label>
              <input id="org-website" name="website" class="form-input" placeholder="https://example.com" />
            </div>

            <div class="form-group">
              <label for="org-email">Contact email</label>
              <input id="org-email" name="email" type="email" class="form-input" placeholder="packages@example.com" />
            </div>

            <button type="submit" class="btn btn-primary">Create organization</button>
          </form>
        </section>
      </div>

      <section class="card settings-section mt-6">
        <div class="settings-token-header">
          <div>
            <h2>API tokens</h2>
            <p class="text-muted settings-copy">Create personal automation tokens and revoke old ones.</p>
          </div>
        </div>

        ${
          createdToken
            ? `
              <div class="alert alert-success">
                <div style="margin-bottom:8px;"><strong>New token created.</strong> Copy it now — it will not be shown again.</div>
                <div class="code-block"><button class="copy-btn" data-copy="created-token">Copy</button><code id="created-token">${escapeHtml(createdToken)}</code></div>
              </div>
            `
            : ''
        }

        <form id="token-form" class="settings-subsection">
          <div class="form-group">
            <label for="token-name">Token name</label>
            <input id="token-name" name="name" class="form-input" placeholder="CI / local development / deploy" required />
          </div>
          <div class="form-group">
            <label for="token-expiry">Expires in days (optional)</label>
            <input id="token-expiry" name="expires_in_days" type="number" min="1" class="form-input" placeholder="30" />
          </div>
          <div class="form-group">
            <label>Scopes</label>
            <div class="settings-scope-grid">
              ${TOKEN_SCOPE_OPTIONS.map(
                (scope) => `
                  <label class="settings-checkbox">
                    <input type="checkbox" name="scope" value="${scope}" ${scope === 'tokens:read' || scope === 'tokens:write' ? 'checked' : ''} />
                    <span>${scope}</span>
                  </label>
                `
              ).join('')}
            </div>
          </div>
          <button type="submit" class="btn btn-primary">Create token</button>
        </form>

        <div class="settings-subsection">
          <h3>Active tokens</h3>
          ${
            tokens.length === 0
              ? `<div class="empty-state"><h3>No tokens yet</h3><p>Create one above for CI, publishing, or local automation.</p></div>`
              : `<div class="token-list">
                  ${tokens
                    .map(
                      (token) => `
                        <div class="token-row">
                          <div class="token-row__main">
                            <div class="token-row__title">${escapeHtml(token.name || 'Unnamed token')}</div>
                            <div class="token-row__meta">
                              <span>${escapeHtml(token.kind || 'personal')}</span>
                              <span>created ${escapeHtml(formatDate(token.created_at))}</span>
                              ${token.last_used_at ? `<span>last used ${escapeHtml(formatDate(token.last_used_at))}</span>` : '<span>never used</span>'}
                              ${token.expires_at ? `<span>expires ${escapeHtml(formatDate(token.expires_at))}</span>` : '<span>no expiry</span>'}
                            </div>
                            <div class="token-row__scopes">${(token.scopes || []).map((scope) => `<span class="badge badge-ecosystem">${escapeHtml(scope)}</span>`).join(' ')}</div>
                          </div>
                          <div class="token-row__actions">
                            <button class="btn btn-secondary btn-sm" data-copy-token-prefix="${escapeHtml(token.prefix || 'pub_')}" type="button">Copy prefix</button>
                            <button class="btn btn-danger btn-sm" data-revoke-token="${escapeHtml(token.id || '')}" type="button">Revoke</button>
                          </div>
                        </div>
                      `
                    )
                    .join('')}
                </div>`
          }
        </div>
      </section>
    </div>
  `;

  const profileForm = container.querySelector<HTMLFormElement>('#profile-form');
  profileForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const formData = new FormData(profileForm);
    const submitButton = getSubmitButton(profileForm);
    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Saving…';

    try {
      await updateCurrentUser({
        display_name: formData.get('display_name')?.toString().trim() || null,
        avatar_url: formData.get('avatar_url')?.toString().trim() || null,
        website: formData.get('website')?.toString().trim() || null,
        bio: formData.get('bio')?.toString().trim() || null,
      });

      await loadAndRender(container, {
        notice: 'Profile updated successfully.',
      });
    } catch (caughtError: unknown) {
      await loadAndRender(container, {
        error: toErrorMessage(caughtError, 'Failed to update profile.'),
      });
    }
  });

  const tokenForm = container.querySelector<HTMLFormElement>('#token-form');
  tokenForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const formData = new FormData(tokenForm);
    const tokenName = formData.get('name')?.toString().trim() || '';
    const scopes = formData.getAll('scope').map((scope) => scope.toString());
    const expiresRaw = formData.get('expires_in_days')?.toString().trim();
    const submitButton = getSubmitButton(tokenForm);

    if (!submitButton) {
      return;
    }

    if (!tokenName) {
      await loadAndRender(container, {
        error: 'Token name is required.',
        mfaSetupState,
      });
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Creating…';

    try {
      const result = await createToken({
        name: tokenName,
        scopes,
        expires_in_days: expiresRaw ? Number(expiresRaw) : null,
      });

      await loadAndRender(container, {
        notice: 'Token created successfully.',
        createdToken: result.token,
        mfaSetupState,
      });
    } catch (caughtError: unknown) {
      await loadAndRender(container, {
        error: toErrorMessage(caughtError, 'Failed to create token.'),
        mfaSetupState,
      });
    }
  });

  container
    .querySelectorAll<HTMLButtonElement>('[data-revoke-token]')
    .forEach((button) => {
      button.addEventListener('click', async () => {
        const tokenId = button.getAttribute('data-revoke-token');
        if (!tokenId) {
          return;
        }

        button.disabled = true;
        button.textContent = 'Revoking…';

        try {
          await revokeToken(tokenId);
          await loadAndRender(container, {
            notice: 'Token revoked.',
            mfaSetupState,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, {
            error: toErrorMessage(caughtError, 'Failed to revoke token.'),
            mfaSetupState,
          });
        }
      });
    });

  container
    .querySelector<HTMLButtonElement>('#mfa-setup-btn')
    ?.addEventListener('click', async (event) => {
      const button = event.currentTarget;
      if (!(button instanceof HTMLButtonElement)) {
        return;
      }

      button.disabled = true;
      button.textContent = 'Preparing…';

      try {
        const setup = await setupMfa();
        await loadAndRender(container, {
          notice: 'MFA setup initialized. Verify one code to enable it.',
          mfaSetupState: setup,
        });
      } catch (caughtError: unknown) {
        await loadAndRender(container, {
          error: toErrorMessage(caughtError, 'Failed to initialize MFA setup.'),
        });
      }
    });

  const mfaVerifyForm =
    container.querySelector<HTMLFormElement>('#mfa-verify-form');
  mfaVerifyForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const code =
      new FormData(mfaVerifyForm).get('code')?.toString().trim() || '';
    const submitButton = getSubmitButton(mfaVerifyForm);

    if (!submitButton) {
      return;
    }

    if (!code) {
      await loadAndRender(container, {
        error: 'A verification code is required.',
        mfaSetupState,
      });
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Enabling…';

    try {
      await verifyMfaSetup(code);
      await loadAndRender(container, { notice: 'MFA enabled successfully.' });
    } catch (caughtError: unknown) {
      await loadAndRender(container, {
        error: toErrorMessage(caughtError, 'Failed to verify MFA setup.'),
        mfaSetupState,
      });
    }
  });

  const mfaDisableForm =
    container.querySelector<HTMLFormElement>('#mfa-disable-form');
  mfaDisableForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const code =
      new FormData(mfaDisableForm).get('code')?.toString().trim() || '';
    const submitButton = getSubmitButton(mfaDisableForm);

    if (!submitButton) {
      return;
    }

    if (!code) {
      await loadAndRender(container, {
        error: 'A code is required to disable MFA.',
      });
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Disabling…';

    try {
      await disableMfa(code);
      await loadAndRender(container, { notice: 'MFA disabled.' });
    } catch (caughtError: unknown) {
      await loadAndRender(container, {
        error: toErrorMessage(caughtError, 'Failed to disable MFA.'),
      });
    }
  });

  const orgCreateForm =
    container.querySelector<HTMLFormElement>('#org-create-form');
  const orgNameInput =
    orgCreateForm?.querySelector<HTMLInputElement>('input[name="name"]') ??
    null;
  const orgSlugInput =
    orgCreateForm?.querySelector<HTMLInputElement>('input[name="slug"]') ??
    null;
  let orgSlugTouched = false;

  orgSlugInput?.addEventListener('input', () => {
    if (!orgSlugInput) {
      return;
    }

    orgSlugTouched = true;
    orgSlugInput.value = normalizeOrgSlug(orgSlugInput.value);
  });

  orgNameInput?.addEventListener('input', () => {
    if (!orgSlugTouched && orgSlugInput) {
      orgSlugInput.value = normalizeOrgSlug(orgNameInput.value);
    }
  });

  orgCreateForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const formData = new FormData(orgCreateForm);
    const name = formData.get('name')?.toString().trim() || '';
    const slug = normalizeOrgSlug(formData.get('slug')?.toString() || '');

    if (!name || !slug) {
      await loadAndRender(container, {
        error: 'Organization name and a valid slug are required.',
        mfaSetupState,
      });
      return;
    }

    const submitButton = getSubmitButton(orgCreateForm);
    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Creating…';

    try {
      const result = await createOrg({
        name,
        slug,
        description: formData.get('description')?.toString().trim() || null,
        website: formData.get('website')?.toString().trim() || null,
        email: formData.get('email')?.toString().trim() || null,
      });

      await loadAndRender(container, {
        notice: `Organization created successfully. Slug: ${result.slug}.`,
        mfaSetupState,
      });
    } catch (caughtError: unknown) {
      await loadAndRender(container, {
        error: toErrorMessage(caughtError, 'Failed to create organization.'),
        mfaSetupState,
      });
    }
  });

  container
    .querySelectorAll<HTMLButtonElement>('[data-accept-invitation]')
    .forEach((button) => {
      button.addEventListener('click', async () => {
        const invitationId = button.getAttribute('data-accept-invitation');
        if (!invitationId) {
          return;
        }

        button.disabled = true;
        button.textContent = 'Accepting…';

        try {
          const result = await acceptInvitation(invitationId);
          await loadAndRender(container, {
            notice: `Invitation accepted. You are now ${result.role || 'a member'} in ${
              result.org?.name || result.org?.slug || 'the organization'
            }.`,
            mfaSetupState,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, {
            error: toErrorMessage(caughtError, 'Failed to accept invitation.'),
            mfaSetupState,
          });
        }
      });
    });

  container
    .querySelectorAll<HTMLButtonElement>('[data-decline-invitation]')
    .forEach((button) => {
      button.addEventListener('click', async () => {
        const invitationId = button.getAttribute('data-decline-invitation');
        if (!invitationId) {
          return;
        }

        button.disabled = true;
        button.textContent = 'Declining…';

        try {
          await declineInvitation(invitationId);
          await loadAndRender(container, {
            notice: 'Invitation declined.',
            mfaSetupState,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, {
            error: toErrorMessage(caughtError, 'Failed to decline invitation.'),
            mfaSetupState,
          });
        }
      });
    });

  container
    .querySelectorAll<HTMLButtonElement>('[data-copy]')
    .forEach((button) => {
      button.addEventListener('click', async () => {
        const targetId = button.getAttribute('data-copy');
        const text = targetId
          ? container.querySelector<HTMLElement>(`#${targetId}`)?.textContent ||
            ''
          : '';

        const copied = await copyToClipboard(text);
        button.textContent = copied ? 'Copied' : 'Failed';
        window.setTimeout(() => {
          button.textContent = 'Copy';
        }, 1200);
      });
    });

  container
    .querySelectorAll<HTMLButtonElement>('[data-copy-token-prefix]')
    .forEach((button) => {
      button.addEventListener('click', async () => {
        const prefix = button.getAttribute('data-copy-token-prefix') || 'pub_';
        const copied = await copyToClipboard(prefix);
        button.textContent = copied ? 'Copied' : 'Failed';
        window.setTimeout(() => {
          button.textContent = 'Copy prefix';
        }, 1200);
      });
    });
}

function normalizeOrgSlug(value: string): string {
  return value
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9-\s]/g, '')
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-+/, '')
    .slice(0, 64);
}

function getSubmitButton(form: HTMLFormElement): HTMLButtonElement | null {
  return form.querySelector<HTMLButtonElement>('button[type="submit"]');
}

function toErrorMessage(error: unknown, fallback: string): string {
  return error instanceof Error && error.message ? error.message : fallback;
}
