<script lang="ts">
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';

  import type { MfaSetupState, UserProfile } from '../../api/auth';
  import { getAuthToken } from '../../api/client';
  import type { MyInvitation, OrganizationMembership } from '../../api/orgs';
  import type { NamespaceClaim, NamespaceTransferOwnershipResult } from '../../api/namespaces';
  import {
    deleteNamespaceClaim,
    transferNamespaceClaim,
  } from '../../api/namespaces';
  import {
    formatNamespaceClaimStatusLabel,
    sortNamespaceClaims,
  } from '../../pages/personal-namespaces';
  import type { TokenRecord } from '../../api/tokens';
  import SettingsTokenSection from '../../lib/components/SettingsTokenSection.svelte';
  import SettingsMfaSection from '../../lib/components/SettingsMfaSection.svelte';
  import { createSettingsMfaController } from '../../pages/settings-mfa';
  import {
    createSettingsPageController,
    DEFAULT_TOKEN_SCOPES,
    loadSettingsPageState,
  } from '../../pages/settings-page';
  import { formatDate } from '../../utils/format';
  import { ecosystemLabel } from '../../utils/ecosystem';

  const TOKEN_SCOPE_OPTIONS = [
    'profile:write',
    'tokens:read',
    'tokens:write',
    'orgs:write',
    'orgs:join',
    'orgs:transfer',
    'namespaces:write',
    'namespaces:transfer',
    'repositories:write',
    'repositories:transfer',
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
  let namespaceTransferTargets: OrganizationMembership[] = [];
  let organizationsError: string | null = null;
  let namespaceClaims: NamespaceClaim[] = [];
  let namespaceClaimsError: string | null = null;
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
  let selectedScopes = new Set<string>(DEFAULT_TOKEN_SCOPES);
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
    await settingsPageController.initialize();
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
      const loadedState = await loadSettingsPageState({
        toErrorMessage,
      });

      user = loadedState.user;
      tokens = loadedState.tokens;
      organizations = loadedState.organizations;
      namespaceTransferTargets = loadedState.namespaceTransferTargets;
      organizationsError = loadedState.organizationsError;
      namespaceClaims = loadedState.namespaceClaims;
      namespaceClaimsError = loadedState.namespaceClaimsError;
      invitations = loadedState.invitations;
      invitationsError = loadedState.invitationsError;
      displayName = loadedState.displayName;
      avatarUrl = loadedState.avatarUrl;
      website = loadedState.website;
      bio = loadedState.bio;
    } catch (caughtError: unknown) {
      error = toErrorMessage(caughtError, 'Failed to load settings.');
    } finally {
      loading = false;
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

  const settingsPageController = createSettingsPageController({
    getAuthToken,
    gotoLogin: () => goto('/login', { replaceState: true }),
    loadSettings,
    toErrorMessage,
    getMfaSetupState: () => mfaSetupState,
    getDisplayName: () => displayName,
    getAvatarUrl: () => avatarUrl,
    getWebsite: () => website,
    getBio: () => bio,
    setProfileSubmitting: (value) => {
      profileSubmitting = value;
    },
    getTokenName: () => tokenName,
    setTokenName: (value) => {
      tokenName = value;
    },
    getTokenExpiryDays: () => tokenExpiryDays,
    setTokenExpiryDays: (value) => {
      tokenExpiryDays = value;
    },
    getSelectedScopes: () => selectedScopes,
    setSelectedScopes: (value) => {
      selectedScopes = value;
    },
    setCreatingToken: (value) => {
      creatingToken = value;
    },
    getOrgName: () => orgName,
    setOrgName: (value) => {
      orgName = value;
    },
    getOrgSlug: () => orgSlug,
    setOrgSlug: (value) => {
      orgSlug = value;
    },
    getOrgDescription: () => orgDescription,
    setOrgDescription: (value) => {
      orgDescription = value;
    },
    getOrgWebsite: () => orgWebsite,
    setOrgWebsite: (value) => {
      orgWebsite = value;
    },
    getOrgEmail: () => orgEmail,
    setOrgEmail: (value) => {
      orgEmail = value;
    },
    getOrgSlugTouched: () => orgSlugTouched,
    setOrgSlugTouched: (value) => {
      orgSlugTouched = value;
    },
    setCreatingOrganization: (value) => {
      creatingOrganization = value;
    },
  });

  async function handleProfileSubmit(event: SubmitEvent): Promise<void> {
    await settingsPageController.submitProfile(event);
  }

  async function handleTokenSubmit(event: SubmitEvent): Promise<void> {
    await settingsPageController.submitToken(event);
  }

  async function handleRevokeToken(tokenId: string): Promise<void> {
    await settingsPageController.revokeToken(tokenId);
  }

  async function handleDeleteNamespaceClaim(
    claimId: string | null | undefined,
    namespace: string
  ): Promise<void> {
    if (!claimId) {
      await loadSettings({
        error: 'Failed to delete namespace claim because the claim id is unavailable.',
        mfaSetupState,
      });
      return;
    }

    try {
      await deleteNamespaceClaim(claimId);
      await loadSettings({
        notice: `Namespace claim ${namespace} deleted.`,
        mfaSetupState,
      });
    } catch (caughtError: unknown) {
      await loadSettings({
        error: toErrorMessage(caughtError, 'Failed to delete namespace claim.'),
        mfaSetupState,
      });
    }
  }

  async function handleTransferNamespaceClaim(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const formData = new FormData(event.currentTarget as HTMLFormElement);
    const claimId = formData.get('claim_id')?.toString().trim() || '';
    const targetOrgSlug =
      formData.get('target_org_slug')?.toString().trim() || '';

    if (!claimId) {
      await loadSettings({
        error: 'Select a namespace claim to transfer.',
        mfaSetupState,
      });
      return;
    }

    if (!targetOrgSlug) {
      await loadSettings({
        error: 'Select a target organization.',
        mfaSetupState,
      });
      return;
    }

    if (!formData.get('confirm')) {
      await loadSettings({
        error: 'Please confirm the namespace transfer.',
        mfaSetupState,
      });
      return;
    }

    try {
      const result: NamespaceTransferOwnershipResult = await transferNamespaceClaim(
        claimId,
        {
          targetOrgSlug,
        }
      );
      const namespace =
        result.namespace_claim?.namespace ||
        namespaceClaims.find((claim) => claim.id === claimId)?.namespace ||
        'namespace claim';
      await loadSettings({
        notice: `Transferred ${namespace} to ${result.owner?.name || result.owner?.slug || targetOrgSlug}.`,
        mfaSetupState,
      });
    } catch (caughtError: unknown) {
      await loadSettings({
        error: toErrorMessage(
          caughtError,
          'Failed to transfer namespace claim ownership.'
        ),
        mfaSetupState,
      });
    }
  }

  const mfaController = createSettingsMfaController({
    loadSettings,
    toErrorMessage,
    getMfaSetupState: () => mfaSetupState,
    getMfaVerifyCode: () => mfaVerifyCode,
    setMfaVerifyCode: (value) => {
      mfaVerifyCode = value;
    },
    getMfaDisableCode: () => mfaDisableCode,
    setMfaDisableCode: (value) => {
      mfaDisableCode = value;
    },
    setStartingMfaSetup: (value) => {
      startingMfaSetup = value;
    },
    setVerifyingMfa: (value) => {
      verifyingMfa = value;
    },
    setDisablingMfa: (value) => {
      disablingMfa = value;
    },
  });

  async function handleStartMfaSetup(): Promise<void> {
    await mfaController.startSetup();
  }

  async function handleVerifyMfa(event: SubmitEvent): Promise<void> {
    await mfaController.verify(event);
  }

  async function handleDisableMfa(event: SubmitEvent): Promise<void> {
    await mfaController.disable(event);
  }

  function handleOrgNameInput(value: string): void {
    settingsPageController.handleOrgNameInput(value);
  }

  function handleOrgSlugInput(value: string): void {
    settingsPageController.handleOrgSlugInput(value);
  }

  function handleOrgNameInputEvent(event: Event): void {
    handleOrgNameInput((event.currentTarget as HTMLInputElement).value);
  }

  function handleOrgSlugInputEvent(event: Event): void {
    handleOrgSlugInput((event.currentTarget as HTMLInputElement).value);
  }

  async function handleCreateOrganization(event: SubmitEvent): Promise<void> {
    await settingsPageController.createOrganization(event);
  }

  async function handleAcceptInvitation(invitationId: string): Promise<void> {
    await settingsPageController.acceptInvitation(invitationId);
  }

  async function handleDeclineInvitation(invitationId: string): Promise<void> {
    await settingsPageController.declineInvitation(invitationId);
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

      <SettingsMfaSection
        {user}
        {mfaSetupState}
        bind:mfaDisableCode
        {disablingMfa}
        {startingMfaSetup}
        bind:mfaVerifyCode
        {verifyingMfa}
        {handleStartMfaSetup}
        {handleVerifyMfa}
        {handleDisableMfa}
      />
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
        <h2>Your namespace claims</h2>
        <p class="text-muted settings-copy">
          Personal namespace reservations you can revoke when they are no longer
          needed.
        </p>

        {#if namespaceClaimsError}
          <div class="alert alert-error">{namespaceClaimsError}</div>
        {:else if namespaceClaims.length === 0}
          <div class="empty-state">
            <h3>No personal namespace claims</h3>
            <p>
              Namespace claims you own directly will appear here for follow-up
              management.
            </p>
          </div>
        {:else}
          <div class="token-list">
            {#each sortNamespaceClaims(namespaceClaims) as claim}
              <div class="token-row">
                <div class="token-row__main">
                  <div class="token-row__title">
                    {claim.namespace || 'Unnamed claim'}
                  </div>
                  <div class="token-row__meta">
                    <span>{ecosystemLabel(claim.ecosystem)}</span>
                    {#if claim.created_at}<span
                        >created {formatDate(claim.created_at)}</span
                      >{/if}
                  </div>
                </div>
                <div class="token-row__actions">
                  <span
                    class={claim.is_verified
                      ? 'badge badge-verified'
                      : 'badge badge-ecosystem'}
                    >{formatNamespaceClaimStatusLabel(claim)}</span
                  >
                  {#if claim.id}
                    <button
                      class="btn btn-danger btn-sm"
                      type="button"
                      on:click={() =>
                        handleDeleteNamespaceClaim(
                          claim.id,
                          claim.namespace || 'this claim'
                        )}>Delete</button
                    >
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        {/if}

        {#if namespaceClaims.length > 0}
          <div class="settings-subsection">
            <h3>Transfer a personal namespace</h3>
            <p class="settings-copy">
              Move a personal namespace claim into an organization you already
              administer.
            </p>
            {#if namespaceTransferTargets.length === 0}
              <p class="settings-copy">
                Join or create an organization where you are an owner or admin to
                transfer one of these claims.
              </p>
            {:else}
              <div class="alert alert-warning" style="margin-bottom:12px;">
                This transfer is immediate and keeps the claim's verification
                state unchanged.
              </div>
              <form on:submit={handleTransferNamespaceClaim}>
                <div class="grid gap-4 xl:grid-cols-2">
                  <div class="form-group">
                    <label for="settings-namespace-transfer-claim"
                      >Namespace claim</label
                    >
                    <select
                      id="settings-namespace-transfer-claim"
                      name="claim_id"
                      class="form-input"
                      required
                    >
                      <option value="">Select a namespace claim</option>
                      {#each sortNamespaceClaims(namespaceClaims) as claim}
                        <option value={claim.id || ''}
                          >{`${claim.namespace || 'Unnamed claim'} · ${ecosystemLabel(claim.ecosystem)}`}</option
                        >
                      {/each}
                    </select>
                  </div>
                  <div class="form-group">
                    <label for="settings-namespace-transfer-target"
                      >Target organization</label
                    >
                    <select
                      id="settings-namespace-transfer-target"
                      name="target_org_slug"
                      class="form-input"
                      required
                    >
                      <option value="">Select an organization</option>
                      {#each namespaceTransferTargets as target}
                        <option value={target.slug || ''}
                          >{target.name ||
                            target.slug ||
                            'Unnamed organization'}</option
                        >
                      {/each}
                    </select>
                  </div>
                </div>
                <div class="form-group" style="margin-bottom:12px;">
                  <label class="flex items-start gap-2">
                    <input type="checkbox" name="confirm" required />
                    <span
                      >I understand this namespace transfer is immediate.</span
                    >
                  </label>
                </div>
                <button type="submit" class="btn btn-danger"
                  >Transfer namespace</button
                >
              </form>
            {/if}
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

    <SettingsTokenSection
      {createdToken}
      bind:tokenName
      bind:tokenExpiryDays
      {selectedScopes}
      tokenScopeOptions={TOKEN_SCOPE_OPTIONS}
      {creatingToken}
      {tokens}
      {handleScopeToggle}
      {handleTokenSubmit}
      {handleRevokeToken}
    />
  </div>
{/if}
