import type { NamespaceClaim } from '../api/namespaces';
import type {
  OrgMember,
  OrgPackageSummary,
  OrgRepositorySummary,
  Team,
  TeamMember,
  TeamMemberListResponse,
  TeamNamespaceAccessGrant,
  TeamNamespaceAccessListResponse,
  TeamNamespaceAccessMutationResult,
  TeamPackageAccessGrant,
  TeamPackageAccessListResponse,
  TeamRepositoryAccessGrant,
  TeamRepositoryAccessListResponse,
} from '../api/orgs';
import type { OrgMemberPickerOption } from './org-member-picker';
import {
  addTeamMember,
  listTeamMembers,
  listTeamNamespaceAccess,
  listTeamPackageAccess,
  listTeamRepositoryAccess,
  removeTeamMember,
  removeTeamNamespaceAccess,
  removeTeamPackageAccess,
  removeTeamRepositoryAccess,
  replaceTeamNamespaceAccess,
  replaceTeamPackageAccess,
  replaceTeamRepositoryAccess,
  updateTeam,
} from '../api/orgs';
import {
  buildOrgMemberPickerOptions,
  resolveOrgMemberPickerInput,
} from './org-member-picker';
import {
  renderPackageSelectionValue,
  resolveTeamNamespaceAccessSubmission,
  resolveTeamPackageAccessSubmission,
  resolveTeamRepositoryAccessSubmission,
} from './org-workspace-actions';
import { sortNamespaceClaims } from './personal-namespaces';
import { ecosystemLabel } from '../utils/ecosystem';
import {
  formatRepositoryKindLabel,
  formatRepositoryVisibilityLabel,
} from '../utils/repositories';

export interface TeamAccessGrantTargetOption {
  value: string;
  label: string;
}

export interface TeamAccessPermissionOption {
  value: string;
  label: string;
  description: string;
}

export interface TeamMemberState {
  members: TeamMember[];
  load_error: string | null;
}

export interface TeamPackageAccessState {
  grants: TeamPackageAccessGrant[];
  load_error: string | null;
}

export interface TeamRepositoryAccessState {
  grants: TeamRepositoryAccessGrant[];
  load_error: string | null;
}

export interface TeamNamespaceAccessState {
  grants: TeamNamespaceAccessGrant[];
  load_error: string | null;
}

export interface SingleTeamManagementState {
  members: TeamMember[];
  membersError: string | null;
  packageAccess: TeamPackageAccessGrant[];
  packageAccessError: string | null;
  repositoryAccess: TeamRepositoryAccessGrant[];
  repositoryAccessError: string | null;
  namespaceAccess: TeamNamespaceAccessGrant[];
  namespaceAccessError: string | null;
}

export interface TeamManagementStateMaps {
  teamMembersBySlug: Record<string, TeamMemberState>;
  teamPackageAccessBySlug: Record<string, TeamPackageAccessState>;
  teamRepositoryAccessBySlug: Record<string, TeamRepositoryAccessState>;
  teamNamespaceAccessBySlug: Record<string, TeamNamespaceAccessState>;
}

export interface TeamManagementLoaders {
  listTeamMembers: typeof listTeamMembers;
  listTeamPackageAccess: typeof listTeamPackageAccess;
  listTeamRepositoryAccess: typeof listTeamRepositoryAccess;
  listTeamNamespaceAccess: typeof listTeamNamespaceAccess;
}

export interface TeamManagementMutations {
  updateTeam: typeof updateTeam;
  addTeamMember: typeof addTeamMember;
  removeTeamMember: typeof removeTeamMember;
  replaceTeamPackageAccess: typeof replaceTeamPackageAccess;
  removeTeamPackageAccess: typeof removeTeamPackageAccess;
  replaceTeamRepositoryAccess: typeof replaceTeamRepositoryAccess;
  removeTeamRepositoryAccess: typeof removeTeamRepositoryAccess;
  replaceTeamNamespaceAccess: typeof replaceTeamNamespaceAccess;
  removeTeamNamespaceAccess: typeof removeTeamNamespaceAccess;
}

export interface TeamManagementReloadOptions {
  notice?: string | null;
  error?: string | null;
}

export interface TeamManagementControllerOptions {
  getOrgSlug: () => string;
  reload: (options?: TeamManagementReloadOptions) => Promise<void>;
  resolveEligibleTeamMemberOptions: (teamSlug: string) => OrgMemberPickerOption[];
  toErrorMessage: (caughtError: unknown, fallback: string) => string;
  mutations?: TeamManagementMutations;
}

const DEFAULT_TEAM_MANAGEMENT_LOADERS: TeamManagementLoaders = {
  listTeamMembers,
  listTeamPackageAccess,
  listTeamRepositoryAccess,
  listTeamNamespaceAccess,
};

const DEFAULT_TEAM_MANAGEMENT_MUTATIONS: TeamManagementMutations = {
  updateTeam,
  addTeamMember,
  removeTeamMember,
  replaceTeamPackageAccess,
  removeTeamPackageAccess,
  replaceTeamRepositoryAccess,
  removeTeamRepositoryAccess,
  replaceTeamNamespaceAccess,
  removeTeamNamespaceAccess,
};

export const TEAM_PERMISSION_OPTIONS: readonly TeamAccessPermissionOption[] = [
  {
    value: 'admin',
    label: 'Admin',
    description: 'Manage package administration workflows.',
  },
  {
    value: 'publish',
    label: 'Publish',
    description: 'Create releases and publish artifacts.',
  },
  {
    value: 'write_metadata',
    label: 'Write metadata',
    description: 'Update package readmes and metadata.',
  },
  {
    value: 'read_private',
    label: 'Read private',
    description: 'Read non-public package data.',
  },
  {
    value: 'security_review',
    label: 'Security review',
    description: 'Reserved for future security workflows.',
  },
  {
    value: 'transfer_ownership',
    label: 'Transfer ownership',
    description: 'Transfer a package to another owner.',
  },
] as const;

export const TEAM_NAMESPACE_PERMISSION_OPTIONS: readonly TeamAccessPermissionOption[] =
  [
    {
      value: 'admin',
      label: 'Admin',
      description: 'Delete organization-owned namespace claims.',
    },
    {
      value: 'transfer_ownership',
      label: 'Transfer ownership',
      description:
        'Transfer a namespace claim into another controlled organization.',
    },
  ] as const;

export function buildRepositoryGrantOptions(
  repositories: OrgRepositorySummary[]
): TeamAccessGrantTargetOption[] {
  return [...repositories]
    .sort((left, right) =>
      `${left.name || left.slug || ''}`.localeCompare(
        `${right.name || right.slug || ''}`
      )
    )
    .map((repository) => ({
      value: repository.slug || '',
      label: `${repository.name || repository.slug || ''} · ${formatRepositoryKindLabel(repository.kind)} · ${formatRepositoryVisibilityLabel(repository.visibility)}`,
    }));
}

export function buildPackageGrantOptions(
  packages: OrgPackageSummary[]
): TeamAccessGrantTargetOption[] {
  return [...packages]
    .sort((left, right) =>
      `${left.ecosystem || ''}:${left.name || ''}`.localeCompare(
        `${right.ecosystem || ''}:${right.name || ''}`
      )
    )
    .map((pkg) => ({
      value: renderPackageSelectionValue(pkg.ecosystem, pkg.name),
      label: `${pkg.ecosystem || ''} · ${pkg.name || ''}`,
    }));
}

export function buildNamespaceGrantOptions(
  claims: NamespaceClaim[]
): TeamAccessGrantTargetOption[] {
  return sortNamespaceClaims(claims)
    .filter(
      (claim): claim is NamespaceClaim & { id: string } =>
        typeof claim.id === 'string' && claim.id.trim().length > 0
    )
    .map((claim) => ({
      value: claim.id,
      label: `${claim.namespace || 'Unnamed claim'} · ${ecosystemLabel(claim.ecosystem)}`,
    }));
}

export function buildEligibleTeamMemberOptions(
  orgMembers: OrgMember[],
  teamMembers: TeamMember[]
): OrgMemberPickerOption[] {
  return buildOrgMemberPickerOptions(
    orgMembers,
    teamMembers.map((member) => member.username?.trim() || '').filter(Boolean)
  );
}

export function formatTeamPermission(permission: string): string {
  return permission
    .split('_')
    .filter(Boolean)
    .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
    .join(' ');
}

export function hasTeamManagementSlug(team: Team): team is Team & { slug: string } {
  return typeof team.slug === 'string' && team.slug.trim().length > 0;
}

async function loadSingleTeamMemberState(
  orgSlug: string,
  teamSlug: string,
  teamName: string,
  toErrorMessage: (caughtError: unknown, fallback: string) => string,
  loaders: TeamManagementLoaders
): Promise<TeamMemberState> {
  try {
    const data: TeamMemberListResponse = await loaders.listTeamMembers(orgSlug, teamSlug);
    return { members: data.members || [], load_error: data.load_error || null };
  } catch (caughtError: unknown) {
    return {
      members: [],
      load_error: toErrorMessage(caughtError, `Failed to load members for ${teamName}.`),
    };
  }
}

async function loadSingleTeamPackageAccessState(
  orgSlug: string,
  teamSlug: string,
  teamName: string,
  toErrorMessage: (caughtError: unknown, fallback: string) => string,
  loaders: TeamManagementLoaders
): Promise<TeamPackageAccessState> {
  try {
    const data: TeamPackageAccessListResponse = await loaders.listTeamPackageAccess(
      orgSlug,
      teamSlug
    );
    return { grants: data.package_access || [], load_error: data.load_error || null };
  } catch (caughtError: unknown) {
    return {
      grants: [],
      load_error: toErrorMessage(
        caughtError,
        `Failed to load package access for ${teamName}.`
      ),
    };
  }
}

async function loadSingleTeamRepositoryAccessState(
  orgSlug: string,
  teamSlug: string,
  teamName: string,
  toErrorMessage: (caughtError: unknown, fallback: string) => string,
  loaders: TeamManagementLoaders
): Promise<TeamRepositoryAccessState> {
  try {
    const data: TeamRepositoryAccessListResponse =
      await loaders.listTeamRepositoryAccess(orgSlug, teamSlug);
    return { grants: data.repository_access || [], load_error: data.load_error || null };
  } catch (caughtError: unknown) {
    return {
      grants: [],
      load_error: toErrorMessage(
        caughtError,
        `Failed to load repository access for ${teamName}.`
      ),
    };
  }
}

async function loadSingleTeamNamespaceAccessState(
  orgSlug: string,
  teamSlug: string,
  teamName: string,
  toErrorMessage: (caughtError: unknown, fallback: string) => string,
  loaders: TeamManagementLoaders
): Promise<TeamNamespaceAccessState> {
  try {
    const data: TeamNamespaceAccessListResponse =
      await loaders.listTeamNamespaceAccess(orgSlug, teamSlug);
    return { grants: data.namespace_access || [], load_error: data.load_error || null };
  } catch (caughtError: unknown) {
    return {
      grants: [],
      load_error: toErrorMessage(
        caughtError,
        `Failed to load namespace access for ${teamName}.`
      ),
    };
  }
}

export async function loadSingleTeamManagementState(
  orgSlug: string,
  team: Pick<Team, 'slug' | 'name'>,
  options: {
    includeRepositoryAccess: boolean;
    includeNamespaceAccess: boolean;
    toErrorMessage: (caughtError: unknown, fallback: string) => string;
    loaders?: TeamManagementLoaders;
  }
): Promise<SingleTeamManagementState> {
  const teamSlug = team.slug?.trim() || '';
  const teamName = team.name?.trim() || teamSlug || 'this team';
  const loaders = options.loaders || DEFAULT_TEAM_MANAGEMENT_LOADERS;

  const [
    membersState,
    packageAccessState,
    repositoryAccessState,
    namespaceAccessState,
  ] = await Promise.all([
    loadSingleTeamMemberState(orgSlug, teamSlug, teamName, options.toErrorMessage, loaders),
    loadSingleTeamPackageAccessState(
      orgSlug,
      teamSlug,
      teamName,
      options.toErrorMessage,
      loaders
    ),
    options.includeRepositoryAccess
      ? loadSingleTeamRepositoryAccessState(
          orgSlug,
          teamSlug,
          teamName,
          options.toErrorMessage,
          loaders
        )
      : Promise.resolve<TeamRepositoryAccessState>({ grants: [], load_error: null }),
    options.includeNamespaceAccess
      ? loadSingleTeamNamespaceAccessState(
          orgSlug,
          teamSlug,
          teamName,
          options.toErrorMessage,
          loaders
        )
      : Promise.resolve<TeamNamespaceAccessState>({ grants: [], load_error: null }),
  ]);

  return {
    members: membersState.members,
    membersError: membersState.load_error,
    packageAccess: packageAccessState.grants,
    packageAccessError: packageAccessState.load_error,
    repositoryAccess: repositoryAccessState.grants,
    repositoryAccessError: repositoryAccessState.load_error,
    namespaceAccess: namespaceAccessState.grants,
    namespaceAccessError: namespaceAccessState.load_error,
  };
}

async function loadStateMapByTeamSlug<TState>(
  teams: Team[],
  loadState: (team: Team & { slug: string }) => Promise<TState>
): Promise<Record<string, TState>> {
  const entries = await Promise.all(
    teams.filter(hasTeamManagementSlug).map(async (team) => [team.slug, await loadState(team)] as const)
  );

  return Object.fromEntries(entries);
}

export async function loadTeamManagementStateMaps(
  orgSlug: string,
  teams: Team[],
  options: {
    includeMembers: boolean;
    includePackageAccess: boolean;
    includeRepositoryAccess: boolean;
    includeNamespaceAccess: boolean;
    toErrorMessage: (caughtError: unknown, fallback: string) => string;
    loaders?: TeamManagementLoaders;
  }
): Promise<TeamManagementStateMaps> {
  const loaders = options.loaders || DEFAULT_TEAM_MANAGEMENT_LOADERS;

  const [
    teamMembersBySlug,
    teamPackageAccessBySlug,
    teamRepositoryAccessBySlug,
    teamNamespaceAccessBySlug,
  ] = await Promise.all([
    options.includeMembers
      ? loadStateMapByTeamSlug(teams, (team) =>
          loadSingleTeamMemberState(
            orgSlug,
            team.slug,
            team.name?.trim() || team.slug,
            options.toErrorMessage,
            loaders
          )
        )
      : Promise.resolve<Record<string, TeamMemberState>>({}),
    options.includePackageAccess
      ? loadStateMapByTeamSlug(teams, (team) =>
          loadSingleTeamPackageAccessState(
            orgSlug,
            team.slug,
            team.name?.trim() || team.slug,
            options.toErrorMessage,
            loaders
          )
        )
      : Promise.resolve<Record<string, TeamPackageAccessState>>({}),
    options.includeRepositoryAccess
      ? loadStateMapByTeamSlug(teams, (team) =>
          loadSingleTeamRepositoryAccessState(
            orgSlug,
            team.slug,
            team.name?.trim() || team.slug,
            options.toErrorMessage,
            loaders
          )
        )
      : Promise.resolve<Record<string, TeamRepositoryAccessState>>({}),
    options.includeNamespaceAccess
      ? loadStateMapByTeamSlug(teams, (team) =>
          loadSingleTeamNamespaceAccessState(
            orgSlug,
            team.slug,
            team.name?.trim() || team.slug,
            options.toErrorMessage,
            loaders
          )
        )
      : Promise.resolve<Record<string, TeamNamespaceAccessState>>({}),
  ]);

  return {
    teamMembersBySlug,
    teamPackageAccessBySlug,
    teamRepositoryAccessBySlug,
    teamNamespaceAccessBySlug,
  };
}

export function createTeamManagementController(options: TeamManagementControllerOptions) {
  const mutations = options.mutations || DEFAULT_TEAM_MANAGEMENT_MUTATIONS;

  return {
    async updateTeam(teamSlug: string, event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const formData = new FormData(event.currentTarget as HTMLFormElement);

      try {
        await mutations.updateTeam(options.getOrgSlug(), teamSlug, {
          name: formData.get('name')?.toString().trim() || '',
          description: formData.get('description')?.toString().trim() || '',
        });
        await options.reload({ notice: `Saved changes to ${teamSlug}.` });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(caughtError, 'Failed to update team.'),
        });
      }
    },

    async addTeamMember(teamSlug: string, event: SubmitEvent): Promise<void> {
      event.preventDefault();
      const form = event.currentTarget as HTMLFormElement;
      const formData = new FormData(form);
      const username = resolveOrgMemberPickerInput(
        formData.get('username')?.toString() || '',
        options.resolveEligibleTeamMemberOptions(teamSlug)
      );

      try {
        await mutations.addTeamMember(options.getOrgSlug(), teamSlug, { username });
        form.reset();
        await options.reload({ notice: `Added a member to ${teamSlug}.` });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(caughtError, 'Failed to add team member.'),
        });
      }
    },

    async removeTeamMember(teamSlug: string, username: string): Promise<void> {
      try {
        await mutations.removeTeamMember(options.getOrgSlug(), teamSlug, username);
        await options.reload({ notice: `Removed @${username} from ${teamSlug}.` });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(caughtError, 'Failed to remove team member.'),
        });
      }
    },

    async replaceTeamPackageAccess(
      teamSlug: string,
      event: SubmitEvent
    ): Promise<void> {
      event.preventDefault();
      const resolution = resolveTeamPackageAccessSubmission(
        new FormData(event.currentTarget as HTMLFormElement)
      );

      if (!resolution.ok) {
        await options.reload({ error: resolution.error });
        return;
      }

      try {
        await mutations.replaceTeamPackageAccess(
          options.getOrgSlug(),
          teamSlug,
          resolution.value.ecosystem,
          resolution.value.name,
          {
            permissions: resolution.value.permissions,
          }
        );
        await options.reload({
          notice: `Saved package access for ${resolution.value.name}.`,
        });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to update package access.'
          ),
        });
      }
    },

    async removeTeamPackageAccess(
      teamSlug: string,
      ecosystem: string,
      packageName: string
    ): Promise<void> {
      try {
        await mutations.removeTeamPackageAccess(
          options.getOrgSlug(),
          teamSlug,
          ecosystem,
          packageName
        );
        await options.reload({ notice: `Revoked package access for ${packageName}.` });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to revoke package access.'
          ),
        });
      }
    },

    async replaceTeamRepositoryAccess(
      teamSlug: string,
      event: SubmitEvent
    ): Promise<void> {
      event.preventDefault();
      const resolution = resolveTeamRepositoryAccessSubmission(
        new FormData(event.currentTarget as HTMLFormElement)
      );

      if (!resolution.ok) {
        await options.reload({ error: resolution.error });
        return;
      }

      try {
        await mutations.replaceTeamRepositoryAccess(
          options.getOrgSlug(),
          teamSlug,
          resolution.value.repositorySlug,
          {
            permissions: resolution.value.permissions,
          }
        );
        await options.reload({
          notice: `Saved repository access for ${resolution.value.repositorySlug}.`,
        });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to update repository access.'
          ),
        });
      }
    },

    async removeTeamRepositoryAccess(
      teamSlug: string,
      repositorySlug: string
    ): Promise<void> {
      try {
        await mutations.removeTeamRepositoryAccess(
          options.getOrgSlug(),
          teamSlug,
          repositorySlug
        );
        await options.reload({ notice: `Revoked repository access for ${repositorySlug}.` });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to revoke repository access.'
          ),
        });
      }
    },

    async replaceTeamNamespaceAccess(
      teamSlug: string,
      event: SubmitEvent
    ): Promise<void> {
      event.preventDefault();
      const resolution = resolveTeamNamespaceAccessSubmission(
        new FormData(event.currentTarget as HTMLFormElement)
      );

      if (!resolution.ok) {
        await options.reload({ error: resolution.error });
        return;
      }

      try {
        const result: TeamNamespaceAccessMutationResult =
          await mutations.replaceTeamNamespaceAccess(
            options.getOrgSlug(),
            teamSlug,
            resolution.value.claimId,
            {
              permissions: resolution.value.permissions,
            }
          );
        await options.reload({
          notice: `Saved namespace access for ${result.namespace_claim?.namespace || 'the selected claim'}.`,
        });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to update namespace access.'
          ),
        });
      }
    },

    async removeTeamNamespaceAccess(
      teamSlug: string,
      claimId: string,
      namespace: string
    ): Promise<void> {
      try {
        await mutations.removeTeamNamespaceAccess(
          options.getOrgSlug(),
          teamSlug,
          claimId
        );
        await options.reload({ notice: `Revoked namespace access for ${namespace}.` });
      } catch (caughtError: unknown) {
        await options.reload({
          error: options.toErrorMessage(
            caughtError,
            'Failed to revoke namespace access.'
          ),
        });
      }
    },
  };
}
