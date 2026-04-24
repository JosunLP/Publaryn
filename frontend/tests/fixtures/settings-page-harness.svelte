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
    type SettingsPageLoaders,
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

  function buildController() {
    return createSettingsPageController({
      getAuthToken: () => authToken,
      gotoLogin: () => goto('/login', { replaceState: true }),
      loadSettings,
      toErrorMessage,
      getMfaSetupState: () => mfaSetupState,
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
      tokenActions,
    });
  }

  queueMicrotask(() => {
    void buildController().initialize();
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

<SettingsTokenSection
  {createdToken}
  bind:tokenName
  bind:tokenExpiryDays
  {selectedScopes}
  tokenScopeOptions={TOKEN_SCOPE_OPTIONS}
  {creatingToken}
  {tokens}
  {handleScopeToggle}
  handleTokenSubmit={(event) => buildController().submitToken(event)}
  handleRevokeToken={(tokenId) => buildController().revokeToken(tokenId)}
/>
