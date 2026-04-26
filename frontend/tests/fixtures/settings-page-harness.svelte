<script lang="ts">
  import { goto } from '$app/navigation';

  import type { MfaSetupState } from '../../src/api/auth';
  import type {
    MyInvitation,
    OrganizationMembership,
  } from '../../src/api/orgs';
  import type { NamespaceClaim } from '../../src/api/namespaces';
  import type { TokenRecord } from '../../src/api/tokens';
  import SettingsTokenSection from '../../src/lib/components/SettingsTokenSection.svelte';
  import {
    createSettingsPageController,
    DEFAULT_TOKEN_SCOPES,
    loadSettingsPageState,
    normalizeSettingsOrgSlug,
    type SettingsPageLoaders,
    type SettingsPageOrganizationActions,
    type SettingsPageProfileActions,
    type SettingsPageTokenActions,
  } from '../../src/pages/settings-page';

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

  export let authToken: string | null = 'test-token';
  export let loaders: SettingsPageLoaders;
  export let tokenActions: SettingsPageTokenActions;
  export let profileActions: SettingsPageProfileActions | undefined = undefined;
  export let organizationActions: SettingsPageOrganizationActions | undefined =
    undefined;

  let loading = true;
  let notice: string | null = null;
  let error: string | null = null;
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
  let tokenName = '';
  let tokenExpiryDays = '';
  let selectedScopes = new Set<string>(DEFAULT_TOKEN_SCOPES);
  let creatingToken = false;
  let profileSubmitting = false;
  let orgName = '';
  let orgSlug = '';
  let orgDescription = '';
  let orgWebsite = '';
  let orgEmail = '';
  let orgSlugTouched = false;
  let creatingOrganization = false;

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
        loaders,
        toErrorMessage,
      });
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

  function toErrorMessage(caughtError: unknown, fallback: string): string {
    return caughtError instanceof Error && caughtError.message
      ? caughtError.message
      : fallback;
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
    getAuthToken: () => authToken,
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
    tokenActions,
    profileActions,
    organizationActions,
  });

  queueMicrotask(() => {
    void settingsPageController.initialize();
  });
</script>

{#if loading}
  <div class="loading">Loading settings…</div>
{/if}

{#if notice}<div class="alert alert-success">{notice}</div>{/if}
{#if error}<div class="alert alert-error">{error}</div>{/if}

<div data-test="settings-metadata" hidden>
  {displayName} {avatarUrl} {website} {bio} {organizations.length}
  {namespaceTransferTargets.length} {organizationsError} {namespaceClaims.length}
  {namespaceClaimsError} {invitations.length} {invitationsError}
</div>

<form id="profile-form" on:submit={(event) => settingsPageController.submitProfile(event)}>
  <input id="settings-display-name" bind:value={displayName} />
  <input id="settings-avatar-url" bind:value={avatarUrl} />
  <input id="settings-website" bind:value={website} />
  <textarea id="settings-bio" bind:value={bio}></textarea>
  <button type="submit" disabled={profileSubmitting}>
    {profileSubmitting ? 'Saving…' : 'Save profile'}
  </button>
</form>

<section>
  {#if invitations.length === 0}
    <div>No pending invitations</div>
  {:else}
    {#each invitations as invitation}
      <div data-test={`invitation-${invitation.id || 'unknown'}`}>
        <span>{invitation.org?.name || invitation.org?.slug || 'Organization'}</span>
        {#if invitation.actionable !== false && invitation.id}
          <button
            type="button"
            on:click={() => settingsPageController.acceptInvitation(invitation.id || '')}
          >
            Accept
          </button>
          <button
            type="button"
            on:click={() => settingsPageController.declineInvitation(invitation.id || '')}
          >
            Decline
          </button>
        {/if}
      </div>
    {/each}
  {/if}
</section>

<form
  id="org-create-form"
  on:submit={(event) => settingsPageController.createOrganization(event)}
>
  <input
    id="org-name"
    value={orgName}
    on:input={(event) =>
      settingsPageController.handleOrgNameInput(
        (event.currentTarget as HTMLInputElement).value
      )}
  />
  <input
    id="org-slug"
    value={orgSlug}
    on:input={(event) =>
      settingsPageController.handleOrgSlugInput(
        (event.currentTarget as HTMLInputElement).value
      )}
  />
  <textarea id="org-description" bind:value={orgDescription}></textarea>
  <input id="org-website" bind:value={orgWebsite} />
  <input id="org-email" bind:value={orgEmail} />
  <button type="submit" disabled={creatingOrganization}>
    {creatingOrganization ? 'Creating…' : 'Create organization'}
  </button>
</form>

<div data-test="org-slug-preview">{normalizeSettingsOrgSlug(orgSlug)}</div>

<SettingsTokenSection
  {createdToken}
  bind:tokenName
  bind:tokenExpiryDays
  {selectedScopes}
  tokenScopeOptions={TOKEN_SCOPE_OPTIONS}
  {creatingToken}
  {tokens}
  {handleScopeToggle}
  handleTokenSubmit={(event) => settingsPageController.submitToken(event)}
  handleRevokeToken={(tokenId) => settingsPageController.revokeToken(tokenId)}
/>
