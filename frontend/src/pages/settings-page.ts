import type { MfaSetupState, UserProfile } from '../api/auth';
import { getCurrentUser } from '../api/auth';
import type {
  MyInvitation,
  MyInvitationListResponse,
  OrganizationListResponse,
  OrganizationMembership,
} from '../api/orgs';
import { listMyInvitations, listMyOrganizations } from '../api/orgs';
import type { NamespaceClaim, NamespaceListResponse } from '../api/namespaces';
import { listUserNamespaces } from '../api/namespaces';
import type {
  CreateTokenResponse,
  TokenListResponse,
  TokenRecord,
} from '../api/tokens';
import { createToken, listTokens, revokeToken } from '../api/tokens';
import { selectNamespaceTransferTargets } from './personal-namespaces';

export const DEFAULT_TOKEN_SCOPES = ['tokens:read', 'tokens:write'] as const;

export interface SettingsPageReloadOptions {
  notice?: string | null;
  error?: string | null;
  createdToken?: string | null;
  mfaSetupState?: MfaSetupState | null;
}

export interface SettingsPageLoaders {
  getCurrentUser: typeof getCurrentUser;
  listTokens: typeof listTokens;
  listMyOrganizations: typeof listMyOrganizations;
  listMyInvitations: typeof listMyInvitations;
  listUserNamespaces: typeof listUserNamespaces;
}

export interface SettingsPageLoadedState {
  user: UserProfile;
  tokens: TokenRecord[];
  organizations: OrganizationMembership[];
  namespaceTransferTargets: OrganizationMembership[];
  organizationsError: string | null;
  namespaceClaims: NamespaceClaim[];
  namespaceClaimsError: string | null;
  invitations: MyInvitation[];
  invitationsError: string | null;
  displayName: string;
  avatarUrl: string;
  website: string;
  bio: string;
}

export interface SettingsPageTokenActions {
  createToken: typeof createToken;
  revokeToken: typeof revokeToken;
}

export interface SettingsPageControllerOptions {
  getAuthToken: () => string | null;
  gotoLogin: () => Promise<void>;
  loadSettings: (options?: SettingsPageReloadOptions) => Promise<void>;
  toErrorMessage: (caughtError: unknown, fallback: string) => string;
  getMfaSetupState: () => MfaSetupState | null;
  getTokenName: () => string;
  setTokenName: (value: string) => void;
  getTokenExpiryDays: () => string;
  setTokenExpiryDays: (value: string) => void;
  getSelectedScopes: () => Set<string>;
  setSelectedScopes: (value: Set<string>) => void;
  setCreatingToken: (value: boolean) => void;
  tokenActions?: SettingsPageTokenActions;
}

const DEFAULT_SETTINGS_PAGE_LOADERS: SettingsPageLoaders = {
  getCurrentUser,
  listTokens,
  listMyOrganizations,
  listMyInvitations,
  listUserNamespaces,
};

const DEFAULT_SETTINGS_PAGE_TOKEN_ACTIONS: SettingsPageTokenActions = {
  createToken,
  revokeToken,
};

export async function loadSettingsPageState(options: {
  toErrorMessage: (caughtError: unknown, fallback: string) => string;
  loaders?: SettingsPageLoaders;
}): Promise<SettingsPageLoadedState> {
  const loaders = options.loaders || DEFAULT_SETTINGS_PAGE_LOADERS;
  const loadedUser = await loaders.getCurrentUser();
  const currentUserId =
    typeof loadedUser.id === 'string' && loadedUser.id.trim() ? loadedUser.id : '';

  const [tokenData, organizationData, invitationData, namespaceData] =
    await Promise.all([
      loaders.listTokens(),
      loaders.listMyOrganizations().catch(
        (caughtError: unknown): OrganizationListResponse => ({
          organizations: [],
          load_error: options.toErrorMessage(
            caughtError,
            'Failed to load organizations.'
          ),
        })
      ),
      loaders.listMyInvitations().catch(
        (caughtError: unknown): MyInvitationListResponse => ({
          invitations: [],
          load_error: options.toErrorMessage(
            caughtError,
            'Failed to load invitations.'
          ),
        })
      ),
      currentUserId
        ? loaders.listUserNamespaces(currentUserId).catch(
            (caughtError: unknown): NamespaceListResponse => ({
              namespaces: [],
              load_error: options.toErrorMessage(
                caughtError,
                'Failed to load namespace claims.'
              ),
            })
          )
        : Promise.resolve<NamespaceListResponse>({
            namespaces: [],
            load_error:
              'Failed to load namespace claims because the user id is unavailable.',
          }),
    ]);

  const organizations = organizationData.organizations || [];

  return {
    user: loadedUser,
    tokens: tokenData.tokens || [],
    organizations,
    namespaceTransferTargets: selectNamespaceTransferTargets(
      organizations,
      undefined
    ),
    organizationsError: organizationData.load_error || null,
    namespaceClaims: namespaceData.namespaces || [],
    namespaceClaimsError: namespaceData.load_error || null,
    invitations: invitationData.invitations || [],
    invitationsError: invitationData.load_error || null,
    displayName: loadedUser.display_name || '',
    avatarUrl: loadedUser.avatar_url || '',
    website: loadedUser.website || '',
    bio: loadedUser.bio || '',
  };
}

export function createSettingsPageController(options: SettingsPageControllerOptions) {
  const tokenActions = options.tokenActions || DEFAULT_SETTINGS_PAGE_TOKEN_ACTIONS;

  return {
    async initialize(): Promise<void> {
      if (!options.getAuthToken()) {
        await options.gotoLogin();
        return;
      }

      await options.loadSettings();
    },

    async submitToken(event: SubmitEvent): Promise<void> {
      event.preventDefault();

      const tokenName = String(options.getTokenName() ?? '').trim();
      if (!tokenName) {
        await options.loadSettings({
          error: 'Token name is required.',
          mfaSetupState: options.getMfaSetupState(),
        });
        return;
      }

      options.setCreatingToken(true);

      try {
        const tokenExpiryDays = String(options.getTokenExpiryDays() ?? '').trim();
        const result: CreateTokenResponse = await tokenActions.createToken({
          name: tokenName,
          scopes: [...options.getSelectedScopes()],
          expires_in_days: tokenExpiryDays ? Number(tokenExpiryDays) : null,
        });

        options.setTokenName('');
        options.setTokenExpiryDays('');
        options.setSelectedScopes(new Set(DEFAULT_TOKEN_SCOPES));
        await options.loadSettings({
          notice: 'Token created successfully.',
          createdToken: result.token,
          mfaSetupState: options.getMfaSetupState(),
        });
      } catch (caughtError: unknown) {
        await options.loadSettings({
          error: options.toErrorMessage(caughtError, 'Failed to create token.'),
          mfaSetupState: options.getMfaSetupState(),
        });
      } finally {
        options.setCreatingToken(false);
      }
    },

    async revokeToken(tokenId: string): Promise<void> {
      try {
        await tokenActions.revokeToken(tokenId);
        await options.loadSettings({
          notice: 'Token revoked.',
          mfaSetupState: options.getMfaSetupState(),
        });
      } catch (caughtError: unknown) {
        await options.loadSettings({
          error: options.toErrorMessage(caughtError, 'Failed to revoke token.'),
          mfaSetupState: options.getMfaSetupState(),
        });
      }
    },
  };
}
