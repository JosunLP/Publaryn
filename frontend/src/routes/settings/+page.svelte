<script lang="ts">
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';

  import type { MfaSetupState, UserProfile } from '../../api/auth';
  import {
    disableMfa,
    getCurrentUser,
    setupMfa,
    updateCurrentUser,
    verifyMfaSetup,
  } from '../../api/auth';
  import { getAuthToken } from '../../api/client';
  import type {
    MyInvitation,
    MyInvitationListResponse,
    OrganizationListResponse,
    OrganizationMembership,
  } from '../../api/orgs';
  import {
    acceptInvitation,
    createOrg,
    declineInvitation,
    listMyInvitations,
    listMyOrganizations,
  } from '../../api/orgs';
  import type { TokenRecord } from '../../api/tokens';
  import { createToken, listTokens, revokeToken } from '../../api/tokens';
  import { copyToClipboard, formatDate } from '../../utils/format';

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

  let loading = true;
  let error: string | null = null;
  let notice: string | null = null;

  let user: UserProfile | null = null;
  let tokens: TokenRecord[] = [];
  let organizations: OrganizationMembership[] = [];
  let organizationsError: string | null = null;
  let invitations: MyInvitation[] = [];
  let invitationsError: string | null = null;
  let createdToken: string | null = null;
  let mfaSetupState: MfaSetupState | null = null;

  let displayName = '';
  let avatarUrl = '';
  let website = '';
  let bio = '';
  let profileSubmitting = false;

  let tokenName = '';
  let tokenExpiryDays = '';
  let selectedScopes = new Set<string>(['tokens:read', 'tokens:write']);
  let creatingToken = false;

  let mfaDisableCode = '';
  let disablingMfa = false;
  let startingMfaSetup = false;
  let mfaVerifyCode = '';
  let verifyingMfa = false;

  let orgName = '';
  let orgSlug = '';
  let orgDescription = '';
  let orgWebsite = '';
  let orgEmail = '';
  let orgSlugTouched = false;
  let creatingOrganization = false;

  onMount(async () => {
    if (!getAuthToken()) {
      await goto('/login', { replaceState: true });
      return;
    }

    await loadSettings();
  });

  async function loadSettings(
    options: {
      notice?: string | null;
      error?: string | null;
      createdToken?: string | null;
      mfaSetupState?: MfaSetupState | null;
    } = {}
  ): Promise<void> {
    loading = true;
    notice = options.notice ?? null;
    error = options.error ?? null;
    createdToken = options.createdToken ?? null;
    mfaSetupState = options.mfaSetupState ?? null;

    try {
      const [loadedUser, tokenData, organizationData, invitationData] =
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

      user = loadedUser;
      tokens = tokenData.tokens || [];
      organizations = organizationData.organizations || [];
      organizationsError = organizationData.load_error || null;
      invitations = invitationData.invitations || [];
      invitationsError = invitationData.load_error || null;

      displayName = loadedUser.display_name || '';
      avatarUrl = loadedUser.avatar_url || '';
      website = loadedUser.website || '';
      bio = loadedUser.bio || '';
    } catch (caughtError: unknown) {
      error = toErrorMessage(caughtError, 'Failed to load settings.');
    } finally {
      loading = false;
    }
  }

  async function handleProfileSubmit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    profileSubmitting = true;

    try {
      await updateCurrentUser({
        display_name: optional(displayName),
        avatar_url: optional(avatarUrl),
        website: optional(website),
        bio: optional(bio),
      });

      await loadSettings({
        notice: 'Profile updated successfully.',
        mfaSetupState,
      });
    } catch (caughtError: unknown) {
      await loadSettings({
        error: toErrorMessage(caughtError, 'Failed to update profile.'),
        mfaSetupState,
      });
    } finally {
      profileSubmitting = false;
    }
  }

  function handleScopeToggle(scope: string, checked: boolean): void {
    if (checked) {
      selectedScopes.add(scope);
    } else {
      selectedScopes.delete(scope);
    }
    selectedScopes = new Set(selectedScopes);
  }

  async function handleTokenSubmit(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!tokenName.trim()) {
      await loadSettings({ error: 'Token name is required.', mfaSetupState });
      return;
    }

    creatingToken = true;

    try {
      const result = await createToken({
        name: tokenName.trim(),
        scopes: [...selectedScopes],
        expires_in_days: tokenExpiryDays.trim()
          ? Number(tokenExpiryDays.trim())
          : null,
      });

      tokenName = '';
      tokenExpiryDays = '';
      selectedScopes = new Set(['tokens:read', 'tokens:write']);
      await loadSettings({
        notice: 'Token created successfully.',
        createdToken: result.token,
        mfaSetupState,
      });
    } catch (caughtError: unknown) {
      await loadSettings({
        error: toErrorMessage(caughtError, 'Failed to create token.'),
        mfaSetupState,
      });
    } finally {
      creatingToken = false;
    }
  }

  async function handleRevokeToken(tokenId: string): Promise<void> {
    try {
      await revokeToken(tokenId);
      await loadSettings({ notice: 'Token revoked.', mfaSetupState });
    } catch (caughtError: unknown) {
      await loadSettings({
        error: toErrorMessage(caughtError, 'Failed to revoke token.'),
        mfaSetupState,
      });
    }
  }

  async function handleStartMfaSetup(): Promise<void> {
    startingMfaSetup = true;

    try {
      const setup = await setupMfa();
      await loadSettings({
        notice: 'MFA setup initialized. Verify one code to enable it.',
        mfaSetupState: setup,
      });
    } catch (caughtError: unknown) {
      await loadSettings({
        error: toErrorMessage(caughtError, 'Failed to initialize MFA setup.'),
      });
    } finally {
      startingMfaSetup = false;
    }
  }

  async function handleVerifyMfa(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!mfaVerifyCode.trim()) {
      await loadSettings({
        error: 'A verification code is required.',
        mfaSetupState,
      });
      return;
    }

    verifyingMfa = true;

    try {
      await verifyMfaSetup(mfaVerifyCode.trim());
      mfaVerifyCode = '';
      await loadSettings({ notice: 'MFA enabled successfully.' });
    } catch (caughtError: unknown) {
      await loadSettings({
        error: toErrorMessage(caughtError, 'Failed to verify MFA setup.'),
        mfaSetupState,
      });
    } finally {
      verifyingMfa = false;
    }
  }

  async function handleDisableMfa(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    if (!mfaDisableCode.trim()) {
      await loadSettings({ error: 'A code is required to disable MFA.' });
      return;
    }

    disablingMfa = true;

    try {
      await disableMfa(mfaDisableCode.trim());
      mfaDisableCode = '';
      await loadSettings({ notice: 'MFA disabled.' });
    } catch (caughtError: unknown) {
      await loadSettings({
        error: toErrorMessage(caughtError, 'Failed to disable MFA.'),
      });
    } finally {
      disablingMfa = false;
    }
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

  function handleOrgNameInput(value: string): void {
    orgName = value;
    if (!orgSlugTouched) {
      orgSlug = normalizeOrgSlug(value);
    }
  }

  function handleOrgSlugInput(value: string): void {
    orgSlugTouched = true;
    orgSlug = normalizeOrgSlug(value);
  }

  function handleOrgNameInputEvent(event: Event): void {
    handleOrgNameInput((event.currentTarget as HTMLInputElement).value);
  }

  function handleOrgSlugInputEvent(event: Event): void {
    handleOrgSlugInput((event.currentTarget as HTMLInputElement).value);
  }

  async function handleCreateOrganization(event: SubmitEvent): Promise<void> {
    event.preventDefault();

    const normalizedSlug = normalizeOrgSlug(orgSlug);
    if (!orgName.trim() || !normalizedSlug) {
      await loadSettings({
        error: 'Organization name and a valid slug are required.',
        mfaSetupState,
      });
      return;
    }

    creatingOrganization = true;

    try {
      const result = await createOrg({
        name: orgName.trim(),
        slug: normalizedSlug,
        description: optional(orgDescription),
        website: optional(orgWebsite),
        email: optional(orgEmail),
      });

      orgName = '';
      orgSlug = '';
      orgDescription = '';
      orgWebsite = '';
      orgEmail = '';
      orgSlugTouched = false;
      await loadSettings({
        notice: `Organization created successfully. Slug: ${result.slug}.`,
        mfaSetupState,
      });
    } catch (caughtError: unknown) {
      await loadSettings({
        error: toErrorMessage(caughtError, 'Failed to create organization.'),
        mfaSetupState,
      });
    } finally {
      creatingOrganization = false;
    }
  }

  async function handleAcceptInvitation(invitationId: string): Promise<void> {
    try {
      const result = await acceptInvitation(invitationId);
      await loadSettings({
        notice: `Invitation accepted. You are now ${result.role || 'a member'} in ${
          result.org?.name || result.org?.slug || 'the organization'
        }.`,
        mfaSetupState,
      });
    } catch (caughtError: unknown) {
      await loadSettings({
        error: toErrorMessage(caughtError, 'Failed to accept invitation.'),
        mfaSetupState,
      });
    }
  }

  async function handleDeclineInvitation(invitationId: string): Promise<void> {
    try {
      await declineInvitation(invitationId);
      await loadSettings({ notice: 'Invitation declined.', mfaSetupState });
    } catch (caughtError: unknown) {
      await loadSettings({
        error: toErrorMessage(caughtError, 'Failed to decline invitation.'),
        mfaSetupState,
      });
    }
  }

  function optional(value: string): string | null {
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : null;
  }

  function toErrorMessage(caughtError: unknown, fallback: string): string {
    return caughtError instanceof Error && caughtError.message
      ? caughtError.message
      : fallback;
  }
</script>

<svelte:head>
  <title>Settings — Publaryn</title>
</svelte:head>

{#if loading}
  <div class="loading"><span class="spinner"></span> Loading settings…</div>
{:else if error && !user}
  <div class="mt-6">
    <div class="alert alert-error">{error}</div>
  </div>
{:else if user}
  <div class="mt-6 settings-page">
    <div class="settings-header">
      <div>
        <h1>Settings</h1>
        <p class="text-muted">
          Manage your profile, API tokens, and multi-factor authentication.
        </p>
      </div>
    </div>

    {#if notice}<div class="alert alert-success">{notice}</div>{/if}
    {#if error}<div class="alert alert-error">{error}</div>{/if}

    <div class="settings-grid">
      <section class="card settings-section">
        <h2>Profile</h2>
        <p class="text-muted settings-copy">
          Your public profile information and account details.
        </p>

        <form id="profile-form" on:submit={handleProfileSubmit}>
          <div class="form-group">
            <label for="settings-username">Username</label>
            <input
              id="settings-username"
              class="form-input"
              value={user.username || ''}
              disabled
            />
          </div>
          <div class="form-group">
            <label for="settings-email">Email</label>
            <input
              id="settings-email"
              class="form-input"
              value={user.email || ''}
              disabled
            />
          </div>
          <div class="form-group">
            <label for="settings-display-name">Display name</label>
            <input
              id="settings-display-name"
              bind:value={displayName}
              class="form-input"
            />
          </div>
          <div class="form-group">
            <label for="settings-avatar-url">Avatar URL</label>
            <input
              id="settings-avatar-url"
              bind:value={avatarUrl}
              class="form-input"
            />
          </div>
          <div class="form-group">
            <label for="settings-website">Website</label>
            <input
              id="settings-website"
              bind:value={website}
              class="form-input"
            />
          </div>
          <div class="form-group">
            <label for="settings-bio">Bio</label>
            <textarea
              id="settings-bio"
              bind:value={bio}
              class="form-input"
              rows="4"
            ></textarea>
          </div>
          <button
            type="submit"
            class="btn btn-primary"
            disabled={profileSubmitting}
          >
            {profileSubmitting ? 'Saving…' : 'Save profile'}
          </button>
        </form>
      </section>

      <section class="card settings-section">
        <h2>Multi-factor authentication</h2>
        <p class="text-muted settings-copy">
          Status: <strong>{user.mfa_enabled ? 'Enabled' : 'Disabled'}</strong>
        </p>

        {#if user.mfa_enabled}
          <form id="mfa-disable-form" on:submit={handleDisableMfa}>
            <div class="form-group">
              <label for="mfa-disable-code"
                >Authenticator or recovery code</label
              >
              <input
                id="mfa-disable-code"
                bind:value={mfaDisableCode}
                class="form-input"
                placeholder="123456 or xxxx-yyyy"
                required
              />
            </div>
            <button
              type="submit"
              class="btn btn-danger"
              disabled={disablingMfa}
            >
              {disablingMfa ? 'Disabling…' : 'Disable MFA'}
            </button>
          </form>
        {:else}
          <button
            id="mfa-setup-btn"
            class="btn btn-primary"
            type="button"
            on:click={handleStartMfaSetup}
            disabled={startingMfaSetup}
          >
            {startingMfaSetup ? 'Preparing…' : 'Start MFA setup'}
          </button>
          <p class="text-muted mt-4">
            Use an authenticator app like 1Password, Bitwarden, Authy, or Google
            Authenticator.
          </p>
        {/if}

        {#if mfaSetupState}
          <div class="settings-subsection">
            <h3>Step 1: Add the secret to your authenticator app</h3>
            <div class="code-block">
              <button
                class="copy-btn"
                type="button"
                on:click={() => copyToClipboard(mfaSetupState?.secret || '')}
                >Copy</button
              ><code>{mfaSetupState.secret}</code>
            </div>
            <div class="mt-4">
              <div class="text-muted" style="display:block; margin-bottom:6px;">
                Provisioning URI
              </div>
              <div class="code-block">
                <button
                  class="copy-btn"
                  type="button"
                  on:click={() =>
                    copyToClipboard(mfaSetupState?.provisioning_uri || '')}
                  >Copy</button
                ><code>{mfaSetupState.provisioning_uri}</code>
              </div>
            </div>
            <div class="mt-4">
              <div class="text-muted" style="display:block; margin-bottom:6px;">
                Recovery codes (store these somewhere safe)
              </div>
              <div class="code-block">
                <button
                  class="copy-btn"
                  type="button"
                  on:click={() =>
                    copyToClipboard(
                      mfaSetupState?.recovery_codes.join('\n') || ''
                    )}>Copy</button
                ><code>{mfaSetupState.recovery_codes.join('\n')}</code>
              </div>
            </div>
            <form id="mfa-verify-form" class="mt-4" on:submit={handleVerifyMfa}>
              <div class="form-group">
                <label for="mfa-verify-code"
                  >Step 2: Enter a code from your authenticator app</label
                >
                <input
                  id="mfa-verify-code"
                  bind:value={mfaVerifyCode}
                  class="form-input"
                  placeholder="123456"
                  required
                />
              </div>
              <button
                type="submit"
                class="btn btn-primary"
                disabled={verifyingMfa}
              >
                {verifyingMfa ? 'Enabling…' : 'Enable MFA'}
              </button>
            </form>
          </div>
        {/if}
      </section>
    </div>

    <div class="settings-grid mt-6">
      <section class="card settings-section">
        <h2>Your organizations</h2>
        <p class="text-muted settings-copy">
          Organizations you belong to and the role you currently hold in each
          one.
        </p>

        {#if organizationsError}
          <div class="alert alert-error">{organizationsError}</div>
        {:else if organizations.length === 0}
          <div class="empty-state">
            <h3>No organizations yet</h3>
            <p>
              Create one below or accept an invitation to start collaborating.
            </p>
          </div>
        {:else}
          <div class="token-list">
            {#each organizations as organization}
              <div class="token-row">
                <div class="token-row__main">
                  <div class="token-row__title">
                    {#if organization.slug}
                      <a
                        href={`/orgs/${encodeURIComponent(organization.slug)}`}
                        data-sveltekit-preload-data="hover"
                        >{organization.name ||
                          organization.slug ||
                          'Organization'}</a
                      >
                    {:else}
                      {organization.name || organization.slug || 'Organization'}
                    {/if}
                    {#if organization.is_verified}<span
                        class="badge badge-verified">Verified</span
                      >{/if}
                  </div>
                  <div class="token-row__meta">
                    <span>@{organization.slug || 'unknown'}</span>
                    <span>role {organization.role || 'member'}</span>
                    <span>joined {formatDate(organization.joined_at)}</span>
                  </div>
                  <div class="token-row__scopes">
                    <span class="badge badge-ecosystem"
                      >{String(organization.package_count ?? 0)} packages</span
                    >
                    <span class="badge badge-ecosystem"
                      >{String(organization.team_count ?? 0)} teams</span
                    >
                  </div>
                  {#if organization.description}<p class="settings-copy">
                      {organization.description}
                    </p>{/if}
                </div>
                {#if organization.slug}
                  <div class="token-row__actions">
                    <a
                      class="btn btn-secondary btn-sm"
                      href={`/orgs/${encodeURIComponent(organization.slug)}`}
                      data-sveltekit-preload-data="hover">Open workspace</a
                    >
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </section>

      <section class="card settings-section">
        <h2>Organization invitations</h2>
        <p class="text-muted settings-copy">
          Accept or decline invitations to join organizations.
        </p>

        {#if invitationsError}
          <div class="alert alert-error">{invitationsError}</div>
        {:else if invitations.length === 0}
          <div class="empty-state">
            <h3>No pending invitations</h3>
            <p>
              When an organization invites your account, it will appear here.
            </p>
          </div>
        {:else}
          <div class="token-list">
            {#each invitations as invitation}
              <div class="token-row">
                <div class="token-row__main">
                  <div class="token-row__title">
                    {invitation.org?.name ||
                      invitation.org?.slug ||
                      'Organization'}
                  </div>
                  <div class="token-row__meta">
                    <span>role {invitation.role || 'viewer'}</span>
                    <span
                      >invited by @{invitation.invited_by?.username ||
                        'unknown'}</span
                    >
                    <span>sent {formatDate(invitation.created_at)}</span>
                    <span
                      >{invitation.expires_at
                        ? `expires ${formatDate(invitation.expires_at)}`
                        : 'no expiry'}</span
                    >
                  </div>
                  <div class="token-row__scopes">
                    <span class="badge badge-verified"
                      >{invitation.status || 'pending'}</span
                    >
                  </div>
                </div>
                {#if invitation.actionable !== false && invitation.id}
                  <div class="token-row__actions">
                    <button
                      class="btn btn-primary btn-sm"
                      type="button"
                      on:click={() =>
                        handleAcceptInvitation(invitation.id || '')}
                      >Accept</button
                    >
                    <button
                      class="btn btn-secondary btn-sm"
                      type="button"
                      on:click={() =>
                        handleDeclineInvitation(invitation.id || '')}
                      >Decline</button
                    >
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </section>

      <section class="card settings-section">
        <h2>Create organization</h2>
        <p class="text-muted settings-copy">
          Start a shared workspace for teams, invitations, and delegated package
          governance.
        </p>

        <form id="org-create-form" on:submit={handleCreateOrganization}>
          <div class="form-group">
            <label for="org-name">Organization name</label>
            <input
              id="org-name"
              class="form-input"
              placeholder="Acme"
              required
              value={orgName}
              on:input={handleOrgNameInputEvent}
            />
          </div>

          <div class="form-group">
            <label for="org-slug">Slug</label>
            <input
              id="org-slug"
              class="form-input"
              placeholder="acme"
              pattern={'[a-z0-9][a-z0-9-]{0,63}'}
              required
              value={orgSlug}
              on:input={handleOrgSlugInputEvent}
            />
            <div class="text-muted mt-4">
              Lowercase letters, numbers, and hyphens only. Must start with a
              letter or number.
            </div>
          </div>

          <div class="form-group">
            <label for="org-description">Description</label>
            <textarea
              id="org-description"
              bind:value={orgDescription}
              class="form-input"
              rows="3"
              placeholder="What this organization publishes and maintains"
            ></textarea>
          </div>

          <div class="form-group">
            <label for="org-website">Website</label>
            <input
              id="org-website"
              bind:value={orgWebsite}
              class="form-input"
              placeholder="https://example.com"
            />
          </div>

          <div class="form-group">
            <label for="org-email">Contact email</label>
            <input
              id="org-email"
              bind:value={orgEmail}
              type="email"
              class="form-input"
              placeholder="packages@example.com"
            />
          </div>

          <button
            type="submit"
            class="btn btn-primary"
            disabled={creatingOrganization}
          >
            {creatingOrganization ? 'Creating…' : 'Create organization'}
          </button>
        </form>
      </section>
    </div>

    <section class="card settings-section mt-6">
      <div class="settings-token-header">
        <div>
          <h2>API tokens</h2>
          <p class="text-muted settings-copy">
            Create personal automation tokens and revoke old ones.
          </p>
        </div>
      </div>

      {#if createdToken}
        <div class="alert alert-success">
          <div style="margin-bottom:8px;">
            <strong>New token created.</strong> Copy it now — it will not be shown
            again.
          </div>
          <div class="code-block">
            <button
              class="copy-btn"
              type="button"
              on:click={() => copyToClipboard(createdToken || '')}>Copy</button
            ><code>{createdToken}</code>
          </div>
        </div>
      {/if}

      <form
        id="token-form"
        class="settings-subsection"
        on:submit={handleTokenSubmit}
      >
        <div class="form-group">
          <label for="token-name">Token name</label>
          <input
            id="token-name"
            bind:value={tokenName}
            class="form-input"
            placeholder="CI / local development / deploy"
            required
          />
        </div>
        <div class="form-group">
          <label for="token-expiry">Expires in days (optional)</label>
          <input
            id="token-expiry"
            bind:value={tokenExpiryDays}
            type="number"
            min="1"
            class="form-input"
            placeholder="30"
          />
        </div>
        <div class="form-group">
          <div class="text-sm font-medium">Scopes</div>
          <div class="settings-scope-grid">
            {#each TOKEN_SCOPE_OPTIONS as scope}
              <label class="settings-checkbox">
                <input
                  type="checkbox"
                  checked={selectedScopes.has(scope)}
                  on:change={(event) =>
                    handleScopeToggle(
                      scope,
                      (event.currentTarget as HTMLInputElement).checked
                    )}
                />
                <span>{scope}</span>
              </label>
            {/each}
          </div>
        </div>
        <button type="submit" class="btn btn-primary" disabled={creatingToken}>
          {creatingToken ? 'Creating…' : 'Create token'}
        </button>
      </form>

      <div class="settings-subsection">
        <h3>Active tokens</h3>
        {#if tokens.length === 0}
          <div class="empty-state">
            <h3>No tokens yet</h3>
            <p>Create one above for CI, publishing, or local automation.</p>
          </div>
        {:else}
          <div class="token-list">
            {#each tokens as token}
              <div class="token-row">
                <div class="token-row__main">
                  <div class="token-row__title">
                    {token.name || 'Unnamed token'}
                  </div>
                  <div class="token-row__meta">
                    <span>{token.kind || 'personal'}</span>
                    <span>created {formatDate(token.created_at)}</span>
                    {#if token.last_used_at}<span
                        >last used {formatDate(token.last_used_at)}</span
                      >{:else}<span>never used</span>{/if}
                    {#if token.expires_at}<span
                        >expires {formatDate(token.expires_at)}</span
                      >{:else}<span>no expiry</span>{/if}
                  </div>
                  <div class="token-row__scopes">
                    {#each token.scopes || [] as scope}
                      <span class="badge badge-ecosystem">{scope}</span>
                    {/each}
                  </div>
                </div>
                <div class="token-row__actions">
                  <button
                    class="btn btn-secondary btn-sm"
                    type="button"
                    on:click={() => copyToClipboard(token.prefix || 'pub_')}
                    >Copy prefix</button
                  >
                  {#if token.id}<button
                      class="btn btn-danger btn-sm"
                      type="button"
                      on:click={() => handleRevokeToken(token.id || '')}
                      >Revoke</button
                    >{/if}
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </section>
  </div>
{/if}
