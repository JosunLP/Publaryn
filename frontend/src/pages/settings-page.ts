import type { MfaSetupState, UserProfile, UserUpdate } from '../api/auth';
import { getCurrentUser, updateCurrentUser } from '../api/auth';
import type {
  AcceptInvitationResult,
  CreateOrgInput,
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
import type { NamespaceClaim, NamespaceListResponse } from '../api/namespaces';
import { listUserNamespaces } from '../api/namespaces';
import type {
  CreateTokenResponse,
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

export interface SettingsPageProfileActions {
  updateCurrentUser: typeof updateCurrentUser;
}

export interface SettingsPageOrganizationActions {
  createOrg: typeof createOrg;
  acceptInvitation: typeof acceptInvitation;
  declineInvitation: typeof declineInvitation;
}

export interface SettingsPageControllerOptions {
  getAuthToken: () => string | null;
  gotoLogin: () => Promise<void>;
  loadSettings: (options?: SettingsPageReloadOptions) => Promise<void>;
  toErrorMessage: (caughtError: unknown, fallback: string) => string;
  getMfaSetupState: () => MfaSetupState | null;
  getDisplayName: () => string;
  getAvatarUrl: () => string;
  getWebsite: () => string;
  getBio: () => string;
  setProfileSubmitting: (value: boolean) => void;
  getTokenName: () => string;
  setTokenName: (value: string) => void;
  getTokenExpiryDays: () => string;
  setTokenExpiryDays: (value: string) => void;
  getSelectedScopes: () => Set<string>;
  setSelectedScopes: (value: Set<string>) => void;
  setCreatingToken: (value: boolean) => void;
  getOrgName: () => string;
  setOrgName: (value: string) => void;
  getOrgSlug: () => string;
  setOrgSlug: (value: string) => void;
  getOrgDescription: () => string;
  setOrgDescription: (value: string) => void;
  getOrgWebsite: () => string;
  setOrgWebsite: (value: string) => void;
  getOrgEmail: () => string;
  setOrgEmail: (value: string) => void;
  getOrgSlugTouched: () => boolean;
  setOrgSlugTouched: (value: boolean) => void;
  setCreatingOrganization: (value: boolean) => void;
  tokenActions?: SettingsPageTokenActions;
  profileActions?: SettingsPageProfileActions;
  organizationActions?: SettingsPageOrganizationActions;
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

const DEFAULT_SETTINGS_PAGE_PROFILE_ACTIONS: SettingsPageProfileActions = {
  updateCurrentUser,
};

const DEFAULT_SETTINGS_PAGE_ORGANIZATION_ACTIONS: SettingsPageOrganizationActions = {
  createOrg,
  acceptInvitation,
  declineInvitation,
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
  const profileActions =
    options.profileActions || DEFAULT_SETTINGS_PAGE_PROFILE_ACTIONS;
  const organizationActions =
    options.organizationActions || DEFAULT_SETTINGS_PAGE_ORGANIZATION_ACTIONS;

  return {
    async initialize(): Promise<void> {
      if (!options.getAuthToken()) {
        await options.gotoLogin();
        return;
      }

      await options.loadSettings();
    },

    handleOrgNameInput(value: string): void {
      options.setOrgName(value);
      if (!options.getOrgSlugTouched()) {
        options.setOrgSlug(normalizeSettingsOrgSlug(value));
      }
    },

    handleOrgSlugInput(value: string): void {
      options.setOrgSlugTouched(true);
      options.setOrgSlug(normalizeSettingsOrgSlug(value));
    },

    async submitProfile(event: SubmitEvent): Promise<void> {
      event.preventDefault();
      options.setProfileSubmitting(true);

      try {
        const updates: UserUpdate = {
          display_name: optionalSettingsField(options.getDisplayName()),
          avatar_url: optionalSettingsField(options.getAvatarUrl()),
          website: optionalSettingsField(options.getWebsite()),
          bio: optionalSettingsField(options.getBio()),
        };

        await profileActions.updateCurrentUser(updates);
        await options.loadSettings({
          notice: 'Profile updated successfully.',
          mfaSetupState: options.getMfaSetupState(),
        });
      } catch (caughtError: unknown) {
        await options.loadSettings({
          error: options.toErrorMessage(caughtError, 'Failed to update profile.'),
          mfaSetupState: options.getMfaSetupState(),
        });
      } finally {
        options.setProfileSubmitting(false);
      }
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

    async createOrganization(event: SubmitEvent): Promise<void> {
      event.preventDefault();

      const orgName = String(options.getOrgName() ?? '').trim();
      const normalizedSlug = normalizeSettingsOrgSlug(options.getOrgSlug());
      if (!orgName || !normalizedSlug) {
        await options.loadSettings({
          error: 'Organization name and a valid slug are required.',
          mfaSetupState: options.getMfaSetupState(),
        });
        return;
      }

      options.setCreatingOrganization(true);

      try {
        const result = await organizationActions.createOrg({
          name: orgName,
          slug: normalizedSlug,
          description: optionalSettingsField(options.getOrgDescription()),
          website: optionalSettingsField(options.getOrgWebsite()),
          email: optionalSettingsField(options.getOrgEmail()),
        } satisfies CreateOrgInput);

        options.setOrgName('');
        options.setOrgSlug('');
        options.setOrgDescription('');
        options.setOrgWebsite('');
        options.setOrgEmail('');
        options.setOrgSlugTouched(false);
        await options.loadSettings({
          notice: `Organization created successfully. Slug: ${result.slug}.`,
          mfaSetupState: options.getMfaSetupState(),
        });
      } catch (caughtError: unknown) {
        await options.loadSettings({
          error: options.toErrorMessage(caughtError, 'Failed to create organization.'),
          mfaSetupState: options.getMfaSetupState(),
        });
      } finally {
        options.setCreatingOrganization(false);
      }
    },

    async acceptInvitation(invitationId: string): Promise<void> {
      try {
        const result: AcceptInvitationResult =
          await organizationActions.acceptInvitation(invitationId);
        await options.loadSettings({
          notice: `Invitation accepted. You are now ${result.role || 'a member'} in ${
            result.org?.name || result.org?.slug || 'the organization'
          }.`,
          mfaSetupState: options.getMfaSetupState(),
        });
      } catch (caughtError: unknown) {
        await options.loadSettings({
          error: options.toErrorMessage(
            caughtError,
            'Failed to accept invitation.'
          ),
          mfaSetupState: options.getMfaSetupState(),
        });
      }
    },

    async declineInvitation(invitationId: string): Promise<void> {
      try {
        await organizationActions.declineInvitation(invitationId);
        await options.loadSettings({
          notice: 'Invitation declined.',
          mfaSetupState: options.getMfaSetupState(),
        });
      } catch (caughtError: unknown) {
        await options.loadSettings({
          error: options.toErrorMessage(
            caughtError,
            'Failed to decline invitation.'
          ),
          mfaSetupState: options.getMfaSetupState(),
        });
      }
    },
  };
}

export function optionalSettingsField(value: string): string | null {
  const trimmed = String(value ?? '').trim();
  return trimmed.length > 0 ? trimmed : null;
}

export function normalizeSettingsOrgSlug(value: string): string {
  return String(value ?? '')
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9-\s]/g, '')
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-+/, '')
    .slice(0, 64);
}
