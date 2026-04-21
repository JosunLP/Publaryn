import { api } from './client';
import { collectPaginatedItems } from './pagination';

type NullableString = string | null;

export interface OrganizationCapabilities {
  can_manage?: boolean | null;
  can_manage_invitations?: boolean | null;
  can_manage_members?: boolean | null;
  can_view_member_directory?: boolean | null;
  can_view_audit_log?: boolean | null;
  can_transfer_ownership?: boolean | null;
}

export interface OrganizationDetail {
  id?: NullableString;
  name?: NullableString;
  slug?: NullableString;
  description?: NullableString;
  is_verified?: boolean;
  mfa_required?: boolean;
  website?: NullableString;
  email?: NullableString;
  created_at?: NullableString;
  capabilities?: OrganizationCapabilities | null;
}

export interface OrganizationMembership extends OrganizationDetail {
  role?: NullableString;
  joined_at?: NullableString;
  package_count?: number | null;
  team_count?: number | null;
}

export interface OrganizationListResponse {
  organizations: OrganizationMembership[];
  load_error?: NullableString;
}

export interface OrgMember {
  user_id?: NullableString;
  display_name?: NullableString;
  username?: NullableString;
  role?: NullableString;
  joined_at?: NullableString;
}

export interface MemberListResponse {
  members: OrgMember[];
  load_error?: NullableString;
}

export interface Team {
  name?: NullableString;
  slug?: NullableString;
  description?: NullableString;
  created_at?: NullableString;
}

export interface TeamListResponse {
  teams: Team[];
  load_error?: NullableString;
}

export interface TeamMember {
  display_name?: NullableString;
  username?: NullableString;
  added_at?: NullableString;
}

export interface TeamMemberListResponse {
  members: TeamMember[];
  load_error?: NullableString;
}

export interface TeamPackageAccessTeam {
  id?: NullableString;
  name?: NullableString;
  slug?: NullableString;
}

export interface TeamPackageAccessGrant {
  package_id?: NullableString;
  name?: NullableString;
  normalized_name?: NullableString;
  ecosystem?: NullableString;
  permissions?: string[] | null;
  granted_at?: NullableString;
}

export interface TeamPackageAccessListResponse {
  team?: TeamPackageAccessTeam | null;
  package_access: TeamPackageAccessGrant[];
  load_error?: NullableString;
}

export interface TeamRepositoryAccessGrant {
  repository_id?: NullableString;
  name?: NullableString;
  slug?: NullableString;
  kind?: NullableString;
  visibility?: NullableString;
  permissions?: string[] | null;
  granted_at?: NullableString;
}

export interface TeamRepositoryAccessListResponse {
  team?: TeamPackageAccessTeam | null;
  repository_access: TeamRepositoryAccessGrant[];
  load_error?: NullableString;
}

export interface TeamNamespaceAccessGrant {
  namespace_claim_id?: NullableString;
  ecosystem?: NullableString;
  namespace?: NullableString;
  is_verified?: boolean | null;
  permissions?: string[] | null;
  granted_at?: NullableString;
}

export interface TeamNamespaceAccessListResponse {
  team?: TeamPackageAccessTeam | null;
  namespace_access: TeamNamespaceAccessGrant[];
  load_error?: NullableString;
}

export interface TeamPackageAccessMutationResult {
  message?: NullableString;
  package?: {
    id?: NullableString;
    ecosystem?: NullableString;
    name?: NullableString;
    normalized_name?: NullableString;
  } | null;
  permissions?: string[] | null;
}

export interface TeamRepositoryAccessMutationResult {
  message?: NullableString;
  repository?: {
    id?: NullableString;
    name?: NullableString;
    slug?: NullableString;
    kind?: NullableString;
    visibility?: NullableString;
  } | null;
  permissions?: string[] | null;
}

export interface TeamNamespaceAccessMutationResult {
  message?: NullableString;
  namespace_claim?: {
    id?: NullableString;
    ecosystem?: NullableString;
    namespace?: NullableString;
    is_verified?: boolean | null;
  } | null;
  permissions?: string[] | null;
}

export interface OrgPackageSummary {
  id?: NullableString;
  ecosystem?: NullableString;
  name?: NullableString;
  description?: NullableString;
  download_count?: number | null;
  created_at?: NullableString;
  can_transfer?: boolean | null;
}

export interface OrgPackageListResponse {
  packages: OrgPackageSummary[];
  load_error?: NullableString;
}

export interface OrgRepositorySummary {
  id?: NullableString;
  name?: NullableString;
  slug?: NullableString;
  description?: NullableString;
  kind?: NullableString;
  visibility?: NullableString;
  upstream_url?: NullableString;
  package_count?: number | null;
  created_at?: NullableString;
  can_transfer?: boolean | null;
}

export interface OrgRepositoryListResponse {
  repositories: OrgRepositorySummary[];
  load_error?: NullableString;
}

export interface OrgSecuritySeverityCounts {
  critical?: number | null;
  high?: number | null;
  medium?: number | null;
  low?: number | null;
  info?: number | null;
}

export interface OrgSecuritySummary {
  open_findings?: number | null;
  affected_packages?: number | null;
  severities?: OrgSecuritySeverityCounts | null;
}

export interface OrgSecurityPackageSummary {
  package_id?: NullableString;
  ecosystem?: NullableString;
  name?: NullableString;
  description?: NullableString;
  visibility?: NullableString;
  open_findings?: number | null;
  worst_severity?: NullableString;
  latest_detected_at?: NullableString;
  severities?: OrgSecuritySeverityCounts | null;
  reviewer_teams?:
    | Array<{
        id?: NullableString;
        slug?: NullableString;
        name?: NullableString;
      }>
    | null;
  can_manage_security?: boolean | null;
}

export interface OrgSecurityFindingsResponse {
  summary?: OrgSecuritySummary | null;
  packages: OrgSecurityPackageSummary[];
  load_error?: NullableString;
}

export interface OrgSecurityQuery {
  severities?: string[];
  ecosystem?: string;
  package?: string;
}

export interface OrgAuditLog {
  id?: NullableString;
  action?: NullableString;
  actor_user_id?: NullableString;
  actor_username?: NullableString;
  actor_display_name?: NullableString;
  actor_token_id?: NullableString;
  target_user_id?: NullableString;
  target_username?: NullableString;
  target_display_name?: NullableString;
  target_org_id?: NullableString;
  target_package_id?: NullableString;
  target_release_id?: NullableString;
  metadata?: Record<string, unknown> | null;
  occurred_at?: NullableString;
}

export interface OrgAuditListResponse {
  page?: number | null;
  per_page?: number | null;
  has_next?: boolean | null;
  logs: OrgAuditLog[];
  load_error?: NullableString;
}

export interface OrgAuditQuery {
  action?: string;
  actorUserId?: string;
  occurredFrom?: string;
  occurredUntil?: string;
  page?: number;
  perPage?: number;
}

export interface UserReference {
  id?: NullableString;
  username?: NullableString;
  email?: NullableString;
}

export interface InvitationActorReference {
  id?: NullableString;
  username?: NullableString;
}

export interface OrgInvitation {
  id?: NullableString;
  invited_user?: UserReference | null;
  invited_by?: InvitationActorReference | null;
  role?: NullableString;
  status?: NullableString;
  accepted_by?: NullableString;
  created_at?: NullableString;
  accepted_at?: NullableString;
  declined_by?: NullableString;
  declined_at?: NullableString;
  expires_at?: NullableString;
  revoked_by?: NullableString;
  revoked_at?: NullableString;
}

export interface OrgInvitationListResponse {
  invitations: OrgInvitation[];
  load_error?: NullableString;
}

export interface MyInvitation {
  id?: NullableString;
  org?: {
    name?: NullableString;
    slug?: NullableString;
  } | null;
  role?: NullableString;
  invited_by?: {
    username?: NullableString;
  } | null;
  created_at?: NullableString;
  expires_at?: NullableString;
  status?: NullableString;
  actionable?: boolean | null;
}

export interface MyInvitationListResponse {
  invitations: MyInvitation[];
  load_error?: NullableString;
}

export interface CreateOrgInput {
  name: string;
  slug: string;
  description?: NullableString;
  website?: NullableString;
  email?: NullableString;
}

export interface UpdateOrgInput {
  name?: string;
  description?: NullableString;
  website?: NullableString;
  email?: NullableString;
  mfaRequired?: boolean;
}

export interface AddMemberInput {
  username: string;
  role: string;
}

export interface CreateTeamInput {
  name: string;
  slug: string;
  description?: NullableString;
}

export interface UpdateTeamInput {
  name: string;
  description?: NullableString;
}

export interface AddTeamMemberInput {
  username: string;
}

export interface ReplaceTeamPackageAccessInput {
  permissions: string[];
}

export interface TransferOwnershipInput {
  username: string;
}

export interface TransferOwnershipResult {
  message?: NullableString;
  org?: {
    id?: NullableString;
    slug?: NullableString;
    name?: NullableString;
  } | null;
  previous_owner?: {
    id?: NullableString;
    new_role?: NullableString;
  } | null;
  new_owner?: {
    id?: NullableString;
    username?: NullableString;
    role?: NullableString;
  } | null;
}

export interface SendInvitationInput {
  usernameOrEmail?: string;
  username?: string;
  email?: string;
  role: string;
  expiresInDays?: number;
}

export interface AcceptInvitationResult {
  role?: NullableString;
  org?: {
    name?: NullableString;
    slug?: NullableString;
  } | null;
}

export async function createOrg(
  input: CreateOrgInput
): Promise<OrganizationDetail> {
  const { data } = await api.post<OrganizationDetail>('/v1/orgs', {
    body: {
      name: input.name,
      slug: input.slug,
      description: input.description,
      website: input.website,
      email: input.email,
    },
  });

  return data;
}

export async function listMyOrganizations(): Promise<OrganizationListResponse> {
  const { data } = await api.get<OrganizationListResponse>(
    '/v1/users/me/organizations'
  );

  return data;
}

export async function getOrg(slug: string): Promise<OrganizationDetail> {
  const { data } = await api.get<OrganizationDetail>(`/v1/orgs/${enc(slug)}`);
  return data;
}

export async function updateOrg(
  slug: string,
  updates: UpdateOrgInput
): Promise<OrganizationDetail> {
  const { data } = await api.patch<OrganizationDetail>(
    `/v1/orgs/${enc(slug)}`,
    {
      body: {
        name: updates.name,
        description: updates.description,
        website: updates.website,
        email: updates.email,
        mfa_required: updates.mfaRequired,
      },
    }
  );

  return data;
}

export async function listMembers(slug: string): Promise<MemberListResponse> {
  const { data } = await api.get<MemberListResponse>(
    `/v1/orgs/${enc(slug)}/members`
  );

  return data;
}

export async function searchOrgMembers(
  slug: string,
  query: string,
  limit = 20
): Promise<MemberListResponse> {
  const params = new URLSearchParams();
  params.set('query', query);
  params.set('limit', String(limit));

  const { data } = await api.get<MemberListResponse>(
    `/v1/orgs/${enc(slug)}/members/search?${params.toString()}`
  );

  return data;
}

export async function addMember(
  slug: string,
  input: AddMemberInput
): Promise<MemberListResponse> {
  const { data } = await api.post<MemberListResponse>(
    `/v1/orgs/${enc(slug)}/members`,
    {
      body: {
        username: input.username,
        role: input.role,
      },
    }
  );

  return data;
}

export async function removeMember(
  slug: string,
  username: string
): Promise<void> {
  await api.delete<null>(`/v1/orgs/${enc(slug)}/members/${enc(username)}`);
}

export async function transferOwnership(
  slug: string,
  input: TransferOwnershipInput
): Promise<TransferOwnershipResult> {
  const { data } = await api.post<TransferOwnershipResult>(
    `/v1/orgs/${enc(slug)}/ownership-transfer`,
    {
      body: {
        username: input.username,
      },
    }
  );

  return data;
}

export async function listTeams(slug: string): Promise<TeamListResponse> {
  const { data } = await api.get<TeamListResponse>(
    `/v1/orgs/${enc(slug)}/teams`
  );
  return data;
}

export async function createTeam(
  slug: string,
  input: CreateTeamInput
): Promise<Team> {
  const { data } = await api.post<Team>(`/v1/orgs/${enc(slug)}/teams`, {
    body: {
      name: input.name,
      slug: input.slug,
      description: input.description,
    },
  });

  return data;
}

export async function updateTeam(
  slug: string,
  teamSlug: string,
  input: UpdateTeamInput
): Promise<Team> {
  const { data } = await api.patch<Team>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}`,
    {
      body: {
        name: input.name,
        description: input.description,
      },
    }
  );

  return data;
}

export async function deleteTeam(
  slug: string,
  teamSlug: string
): Promise<Team> {
  const { data } = await api.delete<Team>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}`
  );

  return data;
}

export async function listTeamMembers(
  slug: string,
  teamSlug: string
): Promise<TeamMemberListResponse> {
  const { data } = await api.get<TeamMemberListResponse>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/members`
  );

  return data;
}

export async function addTeamMember(
  slug: string,
  teamSlug: string,
  input: AddTeamMemberInput
): Promise<TeamMemberListResponse> {
  const { data } = await api.post<TeamMemberListResponse>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/members`,
    {
      body: {
        username: input.username,
      },
    }
  );

  return data;
}

export async function removeTeamMember(
  slug: string,
  teamSlug: string,
  username: string
): Promise<TeamMemberListResponse> {
  const { data } = await api.delete<TeamMemberListResponse>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/members/${enc(username)}`
  );

  return data;
}

export async function listTeamPackageAccess(
  slug: string,
  teamSlug: string
): Promise<TeamPackageAccessListResponse> {
  const { data } = await api.get<TeamPackageAccessListResponse>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/package-access`
  );

  return data;
}

export async function replaceTeamPackageAccess(
  slug: string,
  teamSlug: string,
  ecosystem: string,
  packageName: string,
  input: ReplaceTeamPackageAccessInput
): Promise<TeamPackageAccessMutationResult> {
  const { data } = await api.put<TeamPackageAccessMutationResult>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/package-access/${enc(
      ecosystem
    )}/${enc(packageName)}`,
    {
      body: {
        permissions: input.permissions,
      },
    }
  );

  return data;
}

export async function removeTeamPackageAccess(
  slug: string,
  teamSlug: string,
  ecosystem: string,
  packageName: string
): Promise<TeamPackageAccessMutationResult> {
  const { data } = await api.delete<TeamPackageAccessMutationResult>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/package-access/${enc(
      ecosystem
    )}/${enc(packageName)}`
  );

  return data;
}

export async function listTeamRepositoryAccess(
  slug: string,
  teamSlug: string
): Promise<TeamRepositoryAccessListResponse> {
  const { data } = await api.get<TeamRepositoryAccessListResponse>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/repository-access`
  );

  return data;
}

export async function listTeamNamespaceAccess(
  slug: string,
  teamSlug: string
): Promise<TeamNamespaceAccessListResponse> {
  const { data } = await api.get<TeamNamespaceAccessListResponse>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/namespace-access`
  );

  return data;
}

export async function replaceTeamRepositoryAccess(
  slug: string,
  teamSlug: string,
  repositorySlug: string,
  input: ReplaceTeamPackageAccessInput
): Promise<TeamRepositoryAccessMutationResult> {
  const { data } = await api.put<TeamRepositoryAccessMutationResult>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/repository-access/${enc(
      repositorySlug
    )}`,
    {
      body: {
        permissions: input.permissions,
      },
    }
  );

  return data;
}

export async function removeTeamRepositoryAccess(
  slug: string,
  teamSlug: string,
  repositorySlug: string
): Promise<TeamRepositoryAccessMutationResult> {
  const { data } = await api.delete<TeamRepositoryAccessMutationResult>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/repository-access/${enc(
      repositorySlug
    )}`
  );

  return data;
}

export async function replaceTeamNamespaceAccess(
  slug: string,
  teamSlug: string,
  claimId: string,
  input: ReplaceTeamPackageAccessInput
): Promise<TeamNamespaceAccessMutationResult> {
  const { data } = await api.put<TeamNamespaceAccessMutationResult>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/namespace-access/${enc(
      claimId
    )}`,
    {
      body: {
        permissions: input.permissions,
      },
    }
  );

  return data;
}

export async function removeTeamNamespaceAccess(
  slug: string,
  teamSlug: string,
  claimId: string
): Promise<TeamNamespaceAccessMutationResult> {
  const { data } = await api.delete<TeamNamespaceAccessMutationResult>(
    `/v1/orgs/${enc(slug)}/teams/${enc(teamSlug)}/namespace-access/${enc(
      claimId
    )}`
  );

  return data;
}

export async function listOrgPackages(
  slug: string
): Promise<OrgPackageListResponse> {
  const packages = await collectPaginatedItems(async (page, perPage) => {
    const { data } = await api.get<OrgPackageListResponse>(
      `/v1/orgs/${enc(slug)}/packages`,
      {
        query: {
          page,
          per_page: perPage,
        },
      }
    );

    throwOrgCollectionLoadError(data.load_error);

    return data.packages || [];
  });

  return {
    packages,
  };
}

export async function listOrgRepositories(
  slug: string
): Promise<OrgRepositoryListResponse> {
  const repositories = await collectPaginatedItems(async (page, perPage) => {
    const { data } = await api.get<OrgRepositoryListResponse>(
      `/v1/orgs/${enc(slug)}/repositories`,
      {
        query: {
          page,
          per_page: perPage,
        },
      }
    );

    throwOrgCollectionLoadError(data.load_error);

    return data.repositories || [];
  });

  return {
    repositories,
  };
}

export async function listOrgSecurityFindings(
  slug: string,
  query: OrgSecurityQuery = {}
): Promise<OrgSecurityFindingsResponse> {
  const { data } = await api.get<OrgSecurityFindingsResponse>(
    `/v1/orgs/${enc(slug)}/security-findings`,
    {
      query: {
        severity:
          query.severities && query.severities.length > 0
            ? query.severities.join(',')
            : undefined,
        ecosystem: query.ecosystem,
        package: query.package,
      },
    }
  );

  return data;
}

export async function exportOrgSecurityFindingsCsv(
  slug: string,
  query: OrgSecurityQuery = {}
): Promise<string> {
  const { data } = await api.get<string>(
    `/v1/orgs/${enc(slug)}/security-findings/export`,
    {
      headers: {
        Accept: 'text/csv',
      },
      query: {
        severity:
          query.severities && query.severities.length > 0
            ? query.severities.join(',')
            : undefined,
        ecosystem: query.ecosystem,
        package: query.package,
      },
    }
  );

  return data;
}

export async function listOrgAuditLogs(
  slug: string,
  query: OrgAuditQuery = {}
): Promise<OrgAuditListResponse> {
  const { data } = await api.get<OrgAuditListResponse>(
    `/v1/orgs/${enc(slug)}/audit`,
    {
      query: {
        action: query.action,
        actor_user_id: query.actorUserId,
        occurred_from: query.occurredFrom,
        occurred_until: query.occurredUntil,
        page: query.page,
        per_page: query.perPage,
      },
    }
  );

  return data;
}

export async function exportOrgAuditLogsCsv(
  slug: string,
  query: OrgAuditQuery = {}
): Promise<string> {
  const { data } = await api.get<string>(`/v1/orgs/${enc(slug)}/audit/export`, {
    headers: {
      Accept: 'text/csv',
    },
    query: {
      action: query.action,
      actor_user_id: query.actorUserId,
      occurred_from: query.occurredFrom,
      occurred_until: query.occurredUntil,
    },
  });

  return data;
}

export async function sendInvitation(
  slug: string,
  input: SendInvitationInput
): Promise<OrgInvitation> {
  const { data } = await api.post<OrgInvitation>(
    `/v1/orgs/${enc(slug)}/invitations`,
    {
      body: {
        username_or_email:
          input.usernameOrEmail || input.username || input.email,
        role: input.role,
        expires_in_days: input.expiresInDays,
      },
    }
  );

  return data;
}

export async function listOrgInvitations(
  slug: string,
  { includeInactive = false }: { includeInactive?: boolean } = {}
): Promise<OrgInvitationListResponse> {
  const { data } = await api.get<OrgInvitationListResponse>(
    `/v1/orgs/${enc(slug)}/invitations`,
    {
      query: includeInactive ? { include_inactive: 'true' } : undefined,
    }
  );

  return data;
}

export async function revokeInvitation(
  slug: string,
  id: string
): Promise<void> {
  await api.delete<null>(`/v1/orgs/${enc(slug)}/invitations/${enc(id)}`);
}

export async function listMyInvitations(): Promise<MyInvitationListResponse> {
  const { data } = await api.get<MyInvitationListResponse>(
    '/v1/org-invitations'
  );
  return data;
}

export async function acceptInvitation(
  id: string
): Promise<AcceptInvitationResult> {
  const { data } = await api.post<AcceptInvitationResult>(
    `/v1/org-invitations/${enc(id)}/accept`
  );

  return data;
}

export async function declineInvitation(
  id: string
): Promise<Record<string, unknown>> {
  const { data } = await api.post<Record<string, unknown>>(
    `/v1/org-invitations/${enc(id)}/decline`
  );

  return data;
}

function enc(value: string): string {
  return encodeURIComponent(value);
}

function throwOrgCollectionLoadError(loadError: NullableString | undefined): void {
  // The org collection endpoints may surface a load_error payload instead of a
  // transport failure; treat that as a hard failure so paginated callers do not
  // silently return incomplete repository or package lists.
  if (typeof loadError === 'string' && loadError.length > 0) {
    throw new Error(loadError);
  }
}
