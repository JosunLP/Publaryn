import { ApiError, getAuthToken } from '../api/client';
import type { NamespaceClaim, NamespaceListResponse } from '../api/namespaces';
import { createNamespaceClaim, listOrgNamespaces } from '../api/namespaces';
import type {
  MemberListResponse,
  OrgAuditListResponse,
  OrgAuditLog,
  OrgInvitation,
  OrgInvitationListResponse,
  OrgMember,
  OrgPackageListResponse,
  OrgPackageSummary,
  OrgRepositoryListResponse,
  OrgRepositorySummary,
  OrganizationDetail,
  OrganizationListResponse,
  OrganizationMembership,
  Team,
  TeamListResponse,
  TeamMember,
  TeamPackageAccessGrant,
  TeamPackageAccessListResponse,
  TransferOwnershipResult,
} from '../api/orgs';
import {
  addMember,
  addTeamMember,
  createTeam,
  deleteTeam,
  getOrg,
  listMembers,
  listMyOrganizations,
  listOrgAuditLogs,
  listOrgInvitations,
  listOrgPackages,
  listOrgRepositories,
  listTeamMembers,
  listTeamPackageAccess,
  listTeams,
  removeMember,
  removeTeamMember,
  removeTeamPackageAccess,
  replaceTeamPackageAccess,
  revokeInvitation,
  sendInvitation,
  transferOwnership,
  updateOrg,
  updateTeam,
} from '../api/orgs';
import { transferPackageOwnership } from '../api/packages';
import type {
  RepositoryPackageListResponse,
  RepositoryPackageSummary,
} from '../api/repositories';
import {
  createRepository,
  listRepositoryPackages,
  updateRepository,
} from '../api/repositories';
import type { RouteContext } from '../router';
import { navigate } from '../router';
import { ECOSYSTEMS, ecosystemLabel } from '../utils/ecosystem';
import { escapeHtml, formatDate, formatNumber } from '../utils/format';
import {
  selectPackageTransferTargets,
  selectTransferablePackages,
} from '../utils/package-transfer';
import {
  REPOSITORY_KIND_OPTIONS,
  REPOSITORY_VISIBILITY_OPTIONS,
  formatRepositoryKindLabel,
  formatRepositoryPackageCoverageLabel,
  formatRepositoryVisibilityLabel,
} from '../utils/repositories';
import {
  ORG_AUDIT_ACTION_VALUES,
  buildOrgAuditPath,
  formatAuditActorQueryLabel,
  getAuditViewFromQuery,
  normalizeAuditAction,
  normalizeAuditActorUserId,
  normalizeAuditActorUsername,
} from './org-audit-query';

const ADMIN_ROLES = new Set(['owner', 'admin']);
const ORG_AUDIT_PAGE_SIZE = 20;
const DEFAULT_NAMESPACE_ECOSYSTEM = 'npm';
const ORG_ROLE_OPTIONS = [
  { value: 'admin', label: 'Admin' },
  { value: 'maintainer', label: 'Maintainer' },
  { value: 'publisher', label: 'Publisher' },
  { value: 'security_manager', label: 'Security manager' },
  { value: 'auditor', label: 'Auditor' },
  { value: 'billing_manager', label: 'Billing manager' },
  { value: 'viewer', label: 'Viewer' },
] as const;

const TEAM_PERMISSION_OPTIONS = [
  {
    value: 'admin',
    label: 'Admin',
    description: 'Archive packages and manage trusted publishers.',
  },
  {
    value: 'publish',
    label: 'Publish',
    description: 'Create releases and manage package artifacts.',
  },
  {
    value: 'write_metadata',
    label: 'Write metadata',
    description: 'Update package details such as README and metadata.',
  },
  {
    value: 'read_private',
    label: 'Read private',
    description: 'Read-focused grant for future least-privilege workflows.',
  },
  {
    value: 'security_review',
    label: 'Security review',
    description: 'Reserved for upcoming package security workflows.',
  },
  {
    value: 'transfer_ownership',
    label: 'Transfer ownership',
    description: 'Transfer package ownership out of the organization.',
  },
] as const;

interface TeamMemberState {
  members: TeamMember[];
  load_error: string | null;
}

interface TeamPackageAccessState {
  grants: TeamPackageAccessGrant[];
  load_error: string | null;
}

interface RepositoryPackageState {
  packages: RepositoryPackageSummary[];
  load_error: string | null;
}

interface OrgDetailViewState {
  slug: string;
  org: OrganizationDetail;
  notice: string | null;
  error: string | null;
  membership: OrganizationMembership | undefined;
  canAdminister: boolean;
  members: OrgMember[];
  membersError: string | null;
  teams: Team[];
  teamsError: string | null;
  teamMembersBySlug: Record<string, TeamMemberState>;
  teamPackageAccessBySlug: Record<string, TeamPackageAccessState>;
  namespaceClaims: NamespaceClaim[];
  namespaceError: string | null;
  repositories: OrgRepositorySummary[];
  repositoryPackagesBySlug: Record<string, RepositoryPackageState>;
  repositoriesError: string | null;
  packages: OrgPackageSummary[];
  packagesError: string | null;
  packageTransferTargets: OrganizationMembership[];
  auditLogs: OrgAuditLog[];
  auditError: string | null;
  auditAction: string;
  auditActorUserId: string;
  auditActorUsername: string;
  auditPage: number;
  auditHasNext: boolean;
  invitations: OrgInvitation[];
  invitationsError: string | null;
  isAuthenticated: boolean;
  isOwner: boolean;
}

export function orgDetailPage(
  { params, query }: RouteContext,
  container: HTMLElement
): void {
  const slug = params.slug ?? '';
  const auditView = getAuditViewFromQuery(query);

  container.innerHTML = `<div class="loading"><span class="spinner"></span> Loading organization…</div>`;
  void loadAndRender(container, slug, {
    auditAction: auditView.action,
    auditActorUserId: auditView.actorUserId,
    auditActorUsername: auditView.actorUsername,
    auditPage: auditView.page,
  });
}

async function loadAndRender(
  container: HTMLElement,
  slug: string,
  {
    notice = null,
    error = null,
    auditAction = null,
    auditActorUserId = null,
    auditActorUsername = null,
    auditPage = null,
  }: {
    notice?: string | null;
    error?: string | null;
    auditAction?: string | null;
    auditActorUserId?: string | null;
    auditActorUsername?: string | null;
    auditPage?: number | null;
  } = {}
): Promise<void> {
  try {
    const isAuthenticated = Boolean(getAuthToken());
    const currentAuditView = getAuditViewFromQuery(
      new URLSearchParams(window.location.search)
    );
    const resolvedAuditAction = normalizeAuditAction(
      auditAction ?? currentAuditView.action
    );
    const resolvedAuditActorUserId = normalizeAuditActorUserId(
      auditActorUserId ?? currentAuditView.actorUserId
    );
    const resolvedAuditActorUsername = resolvedAuditActorUserId
      ? normalizeAuditActorUsername(
          auditActorUsername ?? currentAuditView.actorUsername
        )
      : '';
    const resolvedAuditPage =
      typeof auditPage === 'number' &&
      Number.isFinite(auditPage) &&
      auditPage > 0
        ? auditPage
        : currentAuditView.page;

    const [
      org,
      memberData,
      teamData,
      repositoryData,
      packageData,
      myOrganizationsData,
    ] = await Promise.all([
      getOrg(slug),
      listMembers(slug).catch(
        (caughtError: unknown): MemberListResponse => ({
          members: [],
          load_error: toErrorMessage(
            caughtError,
            'Failed to load organization members.'
          ),
        })
      ),
      listTeams(slug).catch(
        (caughtError: unknown): TeamListResponse => ({
          teams: [],
          load_error: toErrorMessage(caughtError, 'Failed to load teams.'),
        })
      ),
      listOrgRepositories(slug).catch(
        (caughtError: unknown): OrgRepositoryListResponse => ({
          repositories: [],
          load_error: toErrorMessage(
            caughtError,
            'Failed to load repositories.'
          ),
        })
      ),
      listOrgPackages(slug).catch(
        (caughtError: unknown): OrgPackageListResponse => ({
          packages: [],
          load_error: toErrorMessage(caughtError, 'Failed to load packages.'),
        })
      ),
      isAuthenticated
        ? listMyOrganizations().catch(
            (): OrganizationListResponse => ({ organizations: [] })
          )
        : Promise.resolve<OrganizationListResponse>({ organizations: [] }),
    ]);

    const membership = (myOrganizationsData.organizations || []).find(
      (item) => item.slug === slug
    );
    const canAdminister = ADMIN_ROLES.has(membership?.role || '');
    const packageTransferTargets = selectPackageTransferTargets(
      myOrganizationsData.organizations || [],
      slug
    );

    const [
      invitationData,
      teamMembersBySlug,
      teamPackageAccessBySlug,
      auditData,
      namespaceData,
      repositoryPackagesBySlug,
    ] = await Promise.all([
      canAdminister
        ? listOrgInvitations(slug).catch(
            (caughtError: unknown): OrgInvitationListResponse => ({
              invitations: [],
              load_error: toErrorMessage(
                caughtError,
                'Failed to load invitations.'
              ),
            })
          )
        : Promise.resolve<OrgInvitationListResponse>({
            invitations: [],
            load_error: null,
          }),
      canAdminister
        ? loadTeamMembers(slug, teamData.teams || [])
        : Promise.resolve<Record<string, TeamMemberState>>({}),
      canAdminister
        ? loadTeamPackageAccess(slug, teamData.teams || [])
        : Promise.resolve<Record<string, TeamPackageAccessState>>({}),
      canAdminister
        ? listOrgAuditLogs(slug, {
            action: resolvedAuditAction || undefined,
            actorUserId: resolvedAuditActorUserId || undefined,
            page: resolvedAuditPage,
            perPage: ORG_AUDIT_PAGE_SIZE,
          }).catch(
            (caughtError: unknown): OrgAuditListResponse => ({
              page: resolvedAuditPage,
              per_page: ORG_AUDIT_PAGE_SIZE,
              has_next: false,
              logs: [],
              load_error: toErrorMessage(
                caughtError,
                'Failed to load the organization activity log.'
              ),
            })
          )
        : Promise.resolve<OrgAuditListResponse>({
            page: resolvedAuditPage,
            per_page: ORG_AUDIT_PAGE_SIZE,
            has_next: false,
            logs: [],
            load_error: null,
          }),
      org.id
        ? listOrgNamespaces(org.id).catch(
            (caughtError: unknown): NamespaceListResponse => ({
              namespaces: [],
              load_error: toErrorMessage(
                caughtError,
                'Failed to load namespace claims.'
              ),
            })
          )
        : Promise.resolve<NamespaceListResponse>({
            namespaces: [],
            load_error:
              'Failed to load namespace claims because the organization id is unavailable.',
          }),
      loadRepositoryPackages(repositoryData.repositories || []),
    ]);

    render(container, {
      slug,
      org,
      notice,
      error,
      membership,
      canAdminister,
      members: memberData.members || [],
      membersError: memberData.load_error || null,
      teams: teamData.teams || [],
      teamsError: teamData.load_error || null,
      teamMembersBySlug,
      teamPackageAccessBySlug,
      namespaceClaims: namespaceData.namespaces || [],
      namespaceError: namespaceData.load_error || null,
      repositories: repositoryData.repositories || [],
      repositoryPackagesBySlug,
      repositoriesError: repositoryData.load_error || null,
      packages: packageData.packages || [],
      packagesError: packageData.load_error || null,
      packageTransferTargets,
      auditLogs: auditData.logs || [],
      auditError: auditData.load_error || null,
      auditAction: resolvedAuditAction,
      auditActorUserId: resolvedAuditActorUserId,
      auditActorUsername: resolvedAuditActorUsername,
      auditPage:
        typeof auditData.page === 'number' && auditData.page > 0
          ? auditData.page
          : resolvedAuditPage,
      auditHasNext: auditData.has_next === true,
      invitations: invitationData.invitations || [],
      invitationsError: invitationData.load_error || null,
      isAuthenticated,
      isOwner: membership?.role === 'owner',
    });
  } catch (caughtError: unknown) {
    if (caughtError instanceof ApiError && caughtError.status === 404) {
      container.innerHTML = `
        <div class="empty-state mt-6">
          <h2>Organization not found</h2>
          <p>@${escapeHtml(slug)} does not exist or is no longer available.</p>
          <a href="/search" class="btn btn-primary mt-4">Search packages</a>
        </div>
      `;
      return;
    }

    container.innerHTML = `
      <div class="mt-6">
        <div class="alert alert-error">${escapeHtml(
          toErrorMessage(caughtError, 'Failed to load organization.')
        )}</div>
      </div>
    `;
  }
}

function render(container: HTMLElement, state: OrgDetailViewState): void {
  const {
    slug,
    org,
    notice,
    error,
    membership,
    canAdminister,
    members,
    membersError,
    teams,
    teamsError,
    teamMembersBySlug,
    teamPackageAccessBySlug,
    namespaceClaims,
    namespaceError,
    repositories,
    repositoryPackagesBySlug,
    repositoriesError,
    packages,
    packagesError,
    packageTransferTargets,
    auditLogs,
    auditError,
    auditAction,
    auditActorUserId,
    auditActorUsername,
    auditPage,
    auditHasNext,
    invitations,
    invitationsError,
    isAuthenticated,
    isOwner,
  } = state;

  container.innerHTML = `
    <div class="mt-6 org-page settings-page">
      ${notice ? `<div class="alert alert-success">${escapeHtml(notice)}</div>` : ''}
      ${error ? `<div class="alert alert-error">${escapeHtml(error)}</div>` : ''}

      <section class="card org-hero">
        <div class="org-hero__header">
          <div class="org-hero__copy">
            <div class="org-hero__eyebrow">Organization workspace</div>
            <div class="pkg-header">
              <h1 class="pkg-header__name">${escapeHtml(org.name || slug)}</h1>
              ${org.is_verified ? '<span class="badge badge-verified">Verified</span>' : ''}
            </div>
            <p class="text-muted">@${escapeHtml(org.slug || slug)}</p>
            ${org.description ? `<p class="settings-copy">${escapeHtml(org.description)}</p>` : '<p class="settings-copy">No organization description yet.</p>'}
          </div>

          <div class="org-hero__meta">
            ${org.website ? `<a href="${escapeHtml(org.website)}" target="_blank" rel="noopener noreferrer">${escapeHtml(org.website)}</a>` : ''}
            ${org.email ? `<a href="mailto:${escapeHtml(org.email)}">${escapeHtml(org.email)}</a>` : ''}
            ${org.created_at ? `<span>Created ${escapeHtml(formatDate(org.created_at))}</span>` : ''}
          </div>
        </div>

        <div class="org-kpi-grid">
          <div class="org-kpi">
            <span class="org-kpi__value">${escapeHtml(String(members.length))}</span>
            <span class="org-kpi__label">Members</span>
          </div>
          <div class="org-kpi">
            <span class="org-kpi__value">${escapeHtml(String(teams.length))}</span>
            <span class="org-kpi__label">Teams</span>
          </div>
          <div class="org-kpi">
            <span class="org-kpi__value">${escapeHtml(String(packages.length))}</span>
            <span class="org-kpi__label">Visible packages</span>
          </div>
          <div class="org-kpi">
            <span class="org-kpi__value">${escapeHtml(formatRole(membership?.role || 'public'))}</span>
            <span class="org-kpi__label">Your access</span>
          </div>
        </div>
      </section>

      ${
        canAdminister
          ? `
            <section class="card settings-section">
              <div class="org-section-header">
                <div>
                  <h2>Activity log</h2>
                  <p class="settings-copy">Browse governance activity for this organization with action filters and page-by-page navigation. This view is limited to owners and admins.</p>
                </div>
              </div>

              <form id="org-audit-filter-form" class="settings-subsection" style="padding-top:0;">
                <div class="flex flex-wrap items-end gap-4">
                  <div class="form-group" style="margin-bottom:0; min-width:240px;">
                    <label for="org-audit-action">Filter by action</label>
                    <select id="org-audit-action" name="action" class="form-input">
                      <option value="">All governance events</option>
                      ${renderAuditActionOptions(auditAction)}
                    </select>
                  </div>
                  <button type="submit" class="btn btn-secondary">Apply filter</button>
                  ${
                    auditAction
                      ? '<button type="button" class="btn btn-secondary" data-clear-audit-action-filter>Clear action</button>'
                      : ''
                  }
                  ${
                    auditActorUserId
                      ? `<button type="button" class="btn btn-secondary" data-clear-audit-actor-filter>Clear actor</button>`
                      : ''
                  }
                </div>
                <p class="settings-copy" style="margin-top:0.75rem; margin-bottom:0;">
                  ${escapeHtml(
                    formatAuditFilterSummary(
                      auditPage,
                      auditAction,
                      auditActorUserId,
                      auditActorUsername
                    )
                  )}
                </p>
              </form>

              ${
                auditError
                  ? `<div class="alert alert-error">${escapeHtml(auditError)}</div>`
                  : auditLogs.length === 0
                    ? `<div class="empty-state"><h3>${escapeHtml(
                        auditAction || auditActorUserId || auditPage > 1
                          ? 'No matching activity'
                          : 'No activity yet'
                      )}</h3><p>${escapeHtml(
                        auditAction || auditActorUserId || auditPage > 1
                          ? 'Try clearing the filter or moving back a page to review earlier governance events.'
                          : 'Recent governance events will appear here once members, invitations, teams, and package access change.'
                      )}</p></div>`
                    : `<div class="token-list">${auditLogs
                        .map((log) => renderAuditLogRow(log, auditActorUserId))
                        .join('')}</div>`
              }

              ${
                !auditError && (auditPage > 1 || auditHasNext)
                  ? `<div class="pagination">
                      ${
                        auditPage > 1
                          ? `<button class="btn btn-secondary btn-sm" type="button" data-audit-page="${escapeHtml(String(auditPage - 1))}">← Prev</button>`
                          : ''
                      }
                      <span class="current">Page ${escapeHtml(String(auditPage))}</span>
                      ${
                        auditHasNext
                          ? `<button class="btn btn-secondary btn-sm" type="button" data-audit-page="${escapeHtml(String(auditPage + 1))}">Next →</button>`
                          : ''
                      }
                    </div>`
                  : ''
              }
            </section>
          `
          : ''
      }

      <div class="settings-grid">
        <section class="card settings-section">
          <h2>Your access</h2>
          ${
            membership
              ? `
                <p class="settings-copy">
                  You are a <strong>${escapeHtml(formatRole(membership.role || 'member'))}</strong> in this organization.
                  ${canAdminister ? 'You can manage invitations and direct memberships from this page.' : 'You can view organization details here; broader admin actions require an owner or admin role.'}
                </p>
              `
              : isAuthenticated
                ? `
                  <p class="settings-copy">
                    You are signed in but not currently a member of this organization.
                    Public information remains visible, while private package visibility and admin actions stay restricted.
                  </p>
                `
                : `
                  <p class="settings-copy">
                    You are viewing this organization as a public visitor.
                    <a href="/login">Sign in</a> to access private memberships and org-admin actions.
                  </p>
                `
          }
        </section>

        <section class="card settings-section">
          <h2>About this organization</h2>
          <p class="settings-copy">
            Use this page as the canonical workspace for organization ownership, members, teams, repositories, delegated package access, namespace claims, and visible packages.
            Audit and security dashboards continue to expand on this foundation.
          </p>
        </section>
      </div>

      ${
        canAdminister
          ? `
            <section class="card settings-section">
              <div class="org-section-header">
                <div>
                  <h2>Organization profile</h2>
                  <p class="settings-copy">Update the shared description and contact metadata that appears across this workspace. Changes are recorded in the organization activity log.</p>
                </div>
              </div>

              <form id="org-profile-form">
                <div class="grid gap-4 xl:grid-cols-2">
                  <div class="form-group">
                    <label for="org-profile-name">Organization name</label>
                    <input id="org-profile-name" class="form-input" value="${escapeHtml(org.name || slug)}" disabled />
                  </div>
                  <div class="form-group">
                    <label for="org-profile-slug">Organization slug</label>
                    <input id="org-profile-slug" class="form-input" value="${escapeHtml(org.slug || slug)}" disabled />
                  </div>
                </div>

                <div class="form-group">
                  <label for="org-profile-description">Description</label>
                  <textarea id="org-profile-description" name="description" class="form-input" rows="3" placeholder="Describe what this organization maintains and why developers should trust it.">${escapeHtml(org.description || '')}</textarea>
                </div>

                <div class="grid gap-4 xl:grid-cols-2">
                  <div class="form-group">
                    <label for="org-profile-website">Website</label>
                    <input id="org-profile-website" name="website" class="form-input" type="url" placeholder="https://packages.example.com" value="${escapeHtml(org.website || '')}" />
                  </div>
                  <div class="form-group">
                    <label for="org-profile-email">Contact email</label>
                    <input id="org-profile-email" name="email" class="form-input" type="email" placeholder="registry@example.com" value="${escapeHtml(org.email || '')}" />
                  </div>
                </div>

                <button type="submit" class="btn btn-primary">Save profile</button>
              </form>
            </section>
          `
          : ''
      }

      ${
        canAdminister
          ? `
            <div class="org-admin-grid">
              <section class="card settings-section">
                <h2>Invite a member</h2>
                <p class="settings-copy">Invitations currently resolve against an existing username or email, then require acceptance by the invited user.</p>
                <form id="org-invite-form">
                  <div class="form-group">
                    <label for="org-invite-target">Username or email</label>
                    <input id="org-invite-target" name="username_or_email" class="form-input" placeholder="alice or alice@example.com" required />
                  </div>
                  <div class="form-group">
                    <label for="org-invite-role">Role</label>
                    <select id="org-invite-role" name="role" class="form-input">
                      ${renderRoleOptions('viewer')}
                    </select>
                  </div>
                  <div class="form-group">
                    <label for="org-invite-expiry">Expires in days</label>
                    <input id="org-invite-expiry" name="expires_in_days" type="number" min="1" max="30" class="form-input" value="7" />
                  </div>
                  <button type="submit" class="btn btn-primary">Send invitation</button>
                </form>
              </section>

              <section class="card settings-section">
                <h2>Add existing member directly</h2>
                <p class="settings-copy">This immediately grants membership to an existing user account. Use the ownership transfer section below to hand off the owner role.</p>
                <form id="org-member-form">
                  <div class="form-group">
                    <label for="org-member-username">Username</label>
                    <input id="org-member-username" name="username" class="form-input" placeholder="alice" required />
                  </div>
                  <div class="form-group">
                    <label for="org-member-role">Role</label>
                    <select id="org-member-role" name="role" class="form-input">
                      ${renderRoleOptions('viewer')}
                    </select>
                  </div>
                  <button type="submit" class="btn btn-primary">Add member</button>
                </form>
              </section>
            </div>
          `
          : ''
      }

      ${
        isOwner
          ? `
            <section class="card settings-section">
              <h2>Transfer ownership</h2>
              <div class="alert alert-warning">
                <strong>This action is immediate and cannot be undone from this page.</strong>
                You will be demoted to Admin and the selected member will become the new organization Owner.
              </div>
              <p class="settings-copy">
                Only existing organization members who are not already owners can receive ownership.
                The transfer takes effect in a single transaction — no approval step is required.
              </p>
              <form id="org-transfer-form">
                <div class="form-group">
                  <label for="org-transfer-target">New owner username</label>
                  <input id="org-transfer-target" name="username" class="form-input" placeholder="alice" required />
                </div>
                <div class="form-group">
                  <label class="flex items-start gap-2">
                    <input type="checkbox" id="org-transfer-confirm" name="confirm" required />
                    <span>I understand this action is immediate and irreversible. I will be demoted to Admin.</span>
                  </label>
                </div>
                <button type="submit" class="btn btn-danger">Transfer ownership</button>
              </form>
            </section>
          `
          : ''
      }

      ${
        canAdminister
          ? `
            <section class="card settings-section">
              <div class="org-section-header">
                <div>
                  <h2>Pending invitations</h2>
                  <p class="settings-copy">Review active invitations and revoke them when plans change.</p>
                </div>
              </div>

              ${
                invitationsError
                  ? `<div class="alert alert-error">${escapeHtml(invitationsError)}</div>`
                  : invitations.length === 0
                    ? `<div class="empty-state"><h3>No active invitations</h3><p>New invitations sent from this page will appear here until they are accepted, declined, revoked, or expired.</p></div>`
                    : `<div class="token-list">
                        ${invitations
                          .map(
                            (invitation) => `
                              <div class="token-row">
                                <div class="token-row__main">
                                  <div class="token-row__title">@${escapeHtml(invitation.invited_user?.username || 'unknown')}</div>
                                  <div class="token-row__meta">
                                    <span>${escapeHtml(invitation.invited_user?.email || 'No email')}</span>
                                    <span>role ${escapeHtml(formatRole(invitation.role || 'viewer'))}</span>
                                    <span>invited ${escapeHtml(formatDate(invitation.created_at))}</span>
                                    <span>expires ${escapeHtml(formatDate(invitation.expires_at))}</span>
                                  </div>
                                </div>
                                <div class="token-row__actions">
                                  <button class="btn btn-secondary btn-sm" data-revoke-invitation="${escapeHtml(invitation.id || '')}" type="button">Revoke</button>
                                </div>
                              </div>
                            `
                          )
                          .join('')}
                      </div>`
              }
            </section>
          `
          : ''
      }

      <section class="card settings-section">
        <div class="org-section-header">
          <div>
            <h2>Members</h2>
            <p class="settings-copy">Public organization memberships and their effective organization roles. Owners stay on the dedicated transfer flow; non-owner roles can be updated inline.</p>
          </div>
        </div>

        ${
          membersError
            ? `<div class="alert alert-error">${escapeHtml(membersError)}</div>`
            : members.length === 0
              ? `<div class="empty-state"><h3>No members yet</h3><p>This organization has not added any members yet.</p></div>`
              : `<div class="token-list">
                  ${members
                    .map(
                      (member) => `
                        <div class="token-row">
                          <div class="token-row__main">
                            <div class="token-row__title">${escapeHtml(member.display_name || member.username || 'Unknown member')}</div>
                            <div class="token-row__meta">
                              <span>@${escapeHtml(member.username || 'unknown')}</span>
                              <span>role ${escapeHtml(formatRole(member.role || 'viewer'))}</span>
                              <span>joined ${escapeHtml(formatDate(member.joined_at))}</span>
                            </div>
                          </div>
                          ${
                            canAdminister && member.role !== 'owner'
                              ? `
                                <div class="token-row__actions">
                                  <form
                                    data-update-member-role-form="${escapeHtml(member.username || '')}"
                                    data-current-role="${escapeHtml(member.role || 'viewer')}"
                                    class="flex flex-wrap items-center gap-2"
                                  >
                                    <label for="member-role-${escapeHtml(member.username || 'member')}" class="text-sm text-muted">Role</label>
                                    <select
                                      id="member-role-${escapeHtml(member.username || 'member')}"
                                      name="role"
                                      class="form-input"
                                      style="width:auto; min-width:150px;"
                                    >
                                      ${renderRoleOptions(member.role || 'viewer')}
                                    </select>
                                    <button class="btn btn-secondary btn-sm" type="submit">Save role</button>
                                  </form>
                                  <button class="btn btn-danger btn-sm" data-remove-member="${escapeHtml(member.username || '')}" type="button">Remove</button>
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

      <div class="settings-grid">
        <section class="card settings-section">
          <div class="org-section-header">
            <div>
              <h2>Teams</h2>
              <p class="settings-copy">Create and manage organization teams and their package responsibilities here.</p>
            </div>
          </div>

          ${
            canAdminister
              ? `
                <form id="org-team-create-form" class="settings-subsection">
                  <h3>Create a team</h3>
                  <p class="settings-copy">Use teams to group existing organization members before delegating package responsibilities.</p>
                  <div class="grid gap-4 xl:grid-cols-2">
                    <div class="form-group">
                      <label for="org-team-name">Team name</label>
                      <input id="org-team-name" name="name" class="form-input" placeholder="Release engineering" required />
                    </div>
                    <div class="form-group">
                      <label for="org-team-slug">Team slug</label>
                      <input id="org-team-slug" name="team_slug" class="form-input" placeholder="release-engineering" required />
                    </div>
                  </div>
                  <div class="form-group">
                    <label for="org-team-description">Description</label>
                    <textarea id="org-team-description" name="description" class="form-input" rows="3" placeholder="Owns release preparation, publication, and package lifecycle coordination."></textarea>
                  </div>
                  <button type="submit" class="btn btn-primary">Create team</button>
                </form>
              `
              : ''
          }

          ${
            teamsError
              ? `<div class="alert alert-error">${escapeHtml(teamsError)}</div>`
              : teams.length === 0
                ? `<div class="empty-state"><h3>No teams yet</h3><p>${canAdminister ? 'Create the first team to delegate package work and ownership boundaries more clearly.' : 'Organization administrators can create teams here as the workspace expands.'}</p></div>`
                : `<div class="settings-section">
                    ${teams
                      .map((team) => {
                        const teamSlug = team.slug || '';
                        return renderTeamCard(team, {
                          canAdminister,
                          packages,
                          packagesError,
                          teamMemberState: teamMembersBySlug[teamSlug] || {
                            members: [],
                            load_error: null,
                          },
                          teamPackageAccessState: teamPackageAccessBySlug[
                            teamSlug
                          ] || {
                            grants: [],
                            load_error: null,
                          },
                        });
                      })
                      .join('')}
                  </div>`
          }
        </section>

        <section class="card settings-section">
          <div class="org-section-header">
            <div>
              <h2>Repositories</h2>
              <p class="settings-copy">Review the repositories that belong to this organization, inspect their visible packages, and manage hosted, staging, or mirrored sources from the same workspace.</p>
            </div>
          </div>

          ${renderOrgRepositories(repositories, repositoryPackagesBySlug, repositoriesError, canAdminister)}

          ${canAdminister && org.id ? renderOrgRepositoryCreateForm() : ''}
        </section>

        <section class="card settings-section">
          <div class="org-section-header">
            <div>
              <h2>Namespace claims</h2>
              <p class="settings-copy">Protect ecosystem-specific namespaces for this organization and make ownership clearer for publishers and consumers.</p>
            </div>
          </div>

          ${renderNamespaceClaims(namespaceClaims, namespaceError, canAdminister)}

          ${canAdminister && org.id ? renderNamespaceClaimForm() : ''}
        </section>

        <section class="card settings-section">
          <div class="org-section-header">
            <div>
              <h2>Visible packages</h2>
              <p class="settings-copy">Showing the packages currently visible from this organization. Public visitors see public packages only.</p>
            </div>
          </div>

          ${
            packagesError
              ? `<div class="alert alert-error">${escapeHtml(packagesError)}</div>`
              : packages.length === 0
                ? `<div class="empty-state"><h3>No packages yet</h3><p>No packages are currently visible for this organization.</p></div>`
                : `<div class="token-list">
                    ${packages
                      .map(
                        (pkg) => `
                          <div class="token-row">
                            <div class="token-row__main">
                              <div class="token-row__title"><a href="/packages/${encodeURIComponent(pkg.ecosystem || 'unknown')}/${encodeURIComponent(pkg.name || '')}">${escapeHtml(pkg.name || 'Unnamed package')}</a></div>
                              <div class="token-row__meta">
                                <span>${escapeHtml(pkg.ecosystem || 'unknown')}</span>
                                <span>${escapeHtml(formatNumber(pkg.download_count))} downloads</span>
                                <span>created ${escapeHtml(formatDate(pkg.created_at))}</span>
                              </div>
                              ${pkg.description ? `<p class="settings-copy">${escapeHtml(pkg.description)}</p>` : ''}
                            </div>
                          </div>
                        `
                      )
                      .join('')}
                  </div>`
          }

          ${
            canAdminister
              ? renderOrgPackageTransferForm(
                  slug,
                  packages,
                  packagesError,
                  packageTransferTargets
                )
              : ''
          }
        </section>
      </div>
    </div>
  `;

  const auditFilterForm = container.querySelector<HTMLFormElement>(
    '#org-audit-filter-form'
  );
  auditFilterForm?.addEventListener('submit', (event) => {
    event.preventDefault();

    const formData = new FormData(auditFilterForm);
    navigate(
      buildOrgAuditPath(
        slug,
        {
          action: formData.get('action')?.toString() || '',
          actorUserId: auditActorUserId,
          actorUsername: auditActorUsername,
          page: 1,
        },
        window.location.search
      )
    );
  });

  const clearAuditActionFilterButton =
    container.querySelector<HTMLButtonElement>(
      '[data-clear-audit-action-filter]'
    );
  clearAuditActionFilterButton?.addEventListener('click', () => {
    navigate(
      buildOrgAuditPath(
        slug,
        {
          action: '',
          actorUserId: auditActorUserId,
          actorUsername: auditActorUsername,
          page: 1,
        },
        window.location.search
      )
    );
  });

  const clearAuditActorFilterButton =
    container.querySelector<HTMLButtonElement>(
      '[data-clear-audit-actor-filter]'
    );
  clearAuditActorFilterButton?.addEventListener('click', () => {
    navigate(
      buildOrgAuditPath(
        slug,
        {
          action: auditAction,
          actorUserId: '',
          actorUsername: '',
          page: 1,
        },
        window.location.search
      )
    );
  });

  container
    .querySelectorAll<HTMLButtonElement>('[data-audit-page]')
    .forEach((button) => {
      button.addEventListener('click', () => {
        const nextPage = Number.parseInt(
          button.getAttribute('data-audit-page') || '',
          10
        );

        if (!Number.isFinite(nextPage) || nextPage < 1) {
          return;
        }

        navigate(
          buildOrgAuditPath(
            slug,
            {
              action: auditAction,
              actorUserId: auditActorUserId,
              actorUsername: auditActorUsername,
              page: nextPage,
            },
            window.location.search
          )
        );
      });
    });

  container
    .querySelectorAll<HTMLButtonElement>('[data-audit-actor-user-id]')
    .forEach((button) => {
      button.addEventListener('click', () => {
        const nextActorUserId =
          button.getAttribute('data-audit-actor-user-id') || '';
        const nextActorUsername =
          button.getAttribute('data-audit-actor-username') || '';

        if (!nextActorUserId) {
          return;
        }

        navigate(
          buildOrgAuditPath(
            slug,
            {
              action: auditAction,
              actorUserId: nextActorUserId,
              actorUsername: nextActorUsername,
              page: 1,
            },
            window.location.search
          )
        );
      });
    });

  const inviteForm =
    container.querySelector<HTMLFormElement>('#org-invite-form');
  const profileForm =
    container.querySelector<HTMLFormElement>('#org-profile-form');
  profileForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const formData = new FormData(profileForm);
    const nextDescription = normalizeFormOptionalText(
      formData.get('description')
    );
    const nextWebsite = normalizeFormOptionalText(formData.get('website'));
    const nextEmail = normalizeFormOptionalText(formData.get('email'));

    if (
      nextDescription === normalizeExistingOptionalText(org.description) &&
      nextWebsite === normalizeExistingOptionalText(org.website) &&
      nextEmail === normalizeExistingOptionalText(org.email)
    ) {
      await loadAndRender(container, slug, {
        notice: 'Organization profile is already up to date.',
      });
      return;
    }

    const submitButton = getSubmitButton(profileForm);
    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Saving…';

    try {
      await updateOrg(slug, {
        description: nextDescription,
        website: nextWebsite,
        email: nextEmail,
      });

      await loadAndRender(container, slug, {
        notice: 'Organization profile updated.',
      });
    } catch (caughtError: unknown) {
      await loadAndRender(container, slug, {
        error: toErrorMessage(
          caughtError,
          'Failed to update the organization profile.'
        ),
      });
    }
  });

  inviteForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const formData = new FormData(inviteForm);
    const submitButton = getSubmitButton(inviteForm);
    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Sending…';

    try {
      await sendInvitation(slug, {
        usernameOrEmail:
          formData.get('username_or_email')?.toString().trim() || '',
        role: formData.get('role')?.toString() || 'viewer',
        expiresInDays:
          Number(formData.get('expires_in_days')?.toString().trim() || '7') ||
          7,
      });

      await loadAndRender(container, slug, {
        notice: 'Invitation sent successfully.',
      });
    } catch (caughtError: unknown) {
      await loadAndRender(container, slug, {
        error: toErrorMessage(caughtError, 'Failed to send invitation.'),
      });
    }
  });

  const memberForm =
    container.querySelector<HTMLFormElement>('#org-member-form');
  memberForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const formData = new FormData(memberForm);
    const submitButton = getSubmitButton(memberForm);
    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Adding…';

    try {
      await addMember(slug, {
        username: formData.get('username')?.toString().trim() || '',
        role: formData.get('role')?.toString() || 'viewer',
      });

      await loadAndRender(container, slug, {
        notice: 'Member added successfully.',
      });
    } catch (caughtError: unknown) {
      await loadAndRender(container, slug, {
        error: toErrorMessage(caughtError, 'Failed to add member.'),
      });
    }
  });

  const transferForm =
    container.querySelector<HTMLFormElement>('#org-transfer-form');
  transferForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const formData = new FormData(transferForm);
    const confirmBox = container.querySelector<HTMLInputElement>(
      '#org-transfer-confirm'
    );

    if (!confirmBox?.checked) {
      await loadAndRender(container, slug, {
        error:
          'Please confirm that you understand the ownership transfer is immediate and irreversible.',
      });
      return;
    }

    const submitButton = getSubmitButton(transferForm);
    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Transferring…';

    try {
      const result: TransferOwnershipResult = await transferOwnership(slug, {
        username: formData.get('username')?.toString().trim() || '',
      });

      const newOwner = result.new_owner?.username || 'the selected user';
      await loadAndRender(container, slug, {
        notice: `Ownership transferred to @${newOwner}. You are now an Admin.`,
      });
    } catch (caughtError: unknown) {
      await loadAndRender(container, slug, {
        error: toErrorMessage(
          caughtError,
          'Failed to transfer organization ownership.'
        ),
      });
    }
  });

  const teamCreateForm = container.querySelector<HTMLFormElement>(
    '#org-team-create-form'
  );
  teamCreateForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const formData = new FormData(teamCreateForm);
    const submitButton = getSubmitButton(teamCreateForm);
    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Creating…';

    try {
      await createTeam(slug, {
        name: formData.get('name')?.toString().trim() || '',
        slug: formData.get('team_slug')?.toString().trim() || '',
        description:
          formData.get('description')?.toString().trim() || undefined,
      });

      await loadAndRender(container, slug, {
        notice: 'Team created successfully.',
      });
    } catch (caughtError: unknown) {
      await loadAndRender(container, slug, {
        error: toErrorMessage(caughtError, 'Failed to create team.'),
      });
    }
  });

  container
    .querySelectorAll<HTMLButtonElement>('[data-revoke-invitation]')
    .forEach((button) => {
      button.addEventListener('click', async () => {
        const invitationId = button.getAttribute('data-revoke-invitation');
        if (!invitationId) {
          return;
        }

        button.disabled = true;
        button.textContent = 'Revoking…';

        try {
          await revokeInvitation(slug, invitationId);
          await loadAndRender(container, slug, {
            notice: 'Invitation revoked.',
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, slug, {
            error: toErrorMessage(caughtError, 'Failed to revoke invitation.'),
          });
        }
      });
    });

  container
    .querySelectorAll<HTMLFormElement>('[data-update-member-role-form]')
    .forEach((form) => {
      form.addEventListener('submit', async (event) => {
        event.preventDefault();

        const username = form.getAttribute('data-update-member-role-form');
        if (!username) {
          return;
        }

        const formData = new FormData(form);
        const role = formData.get('role')?.toString().trim() || 'viewer';
        const currentRole =
          form.getAttribute('data-current-role')?.toString().trim() || '';

        if (currentRole === role) {
          await loadAndRender(container, slug, {
            notice: `@${username} already has the ${formatRole(role)} role.`,
          });
          return;
        }

        const submitButton = getSubmitButton(form);
        if (!submitButton) {
          return;
        }

        submitButton.disabled = true;
        submitButton.textContent = 'Saving…';

        try {
          await addMember(slug, {
            username,
            role,
          });

          await loadAndRender(container, slug, {
            notice: `Updated @${username} to ${formatRole(role)}.`,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, slug, {
            error: toErrorMessage(caughtError, 'Failed to update member role.'),
          });
        }
      });
    });

  container
    .querySelectorAll<HTMLButtonElement>('[data-remove-member]')
    .forEach((button) => {
      button.addEventListener('click', async () => {
        const username = button.getAttribute('data-remove-member');
        if (!username) {
          return;
        }

        button.disabled = true;
        button.textContent = 'Removing…';

        try {
          await removeMember(slug, username);
          await loadAndRender(container, slug, {
            notice: `Removed @${username} from the organization.`,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, slug, {
            error: toErrorMessage(caughtError, 'Failed to remove member.'),
          });
        }
      });
    });

  container
    .querySelectorAll<HTMLFormElement>('[data-update-team-form]')
    .forEach((form) => {
      form.addEventListener('submit', async (event) => {
        event.preventDefault();

        const teamSlug = form.getAttribute('data-update-team-form');
        if (!teamSlug) {
          return;
        }

        const formData = new FormData(form);
        const submitButton = getSubmitButton(form);
        if (!submitButton) {
          return;
        }

        submitButton.disabled = true;
        submitButton.textContent = 'Saving…';

        try {
          await updateTeam(slug, teamSlug, {
            name: formData.get('name')?.toString().trim() || '',
            description: formData.get('description')?.toString().trim() ?? '',
          });

          await loadAndRender(container, slug, {
            notice: `Saved changes to ${teamSlug}.`,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, slug, {
            error: toErrorMessage(caughtError, 'Failed to update team.'),
          });
        }
      });
    });

  container
    .querySelectorAll<HTMLButtonElement>('[data-delete-team]')
    .forEach((button) => {
      button.addEventListener('click', async () => {
        const teamSlug = button.getAttribute('data-delete-team');
        if (!teamSlug) {
          return;
        }

        button.disabled = true;
        button.textContent = 'Deleting…';

        try {
          await deleteTeam(slug, teamSlug);
          await loadAndRender(container, slug, {
            notice: `Deleted team ${teamSlug}.`,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, slug, {
            error: toErrorMessage(caughtError, 'Failed to delete team.'),
          });
        }
      });
    });

  container
    .querySelectorAll<HTMLFormElement>('[data-add-team-member-form]')
    .forEach((form) => {
      form.addEventListener('submit', async (event) => {
        event.preventDefault();

        const teamSlug = form.getAttribute('data-add-team-member-form');
        if (!teamSlug) {
          return;
        }

        const formData = new FormData(form);
        const submitButton = getSubmitButton(form);
        if (!submitButton) {
          return;
        }

        submitButton.disabled = true;
        submitButton.textContent = 'Adding…';

        try {
          await addTeamMember(slug, teamSlug, {
            username: formData.get('username')?.toString().trim() || '',
          });

          await loadAndRender(container, slug, {
            notice: `Added team member to ${teamSlug}.`,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, slug, {
            error: toErrorMessage(caughtError, 'Failed to add team member.'),
          });
        }
      });
    });

  container
    .querySelectorAll<HTMLFormElement>(
      '[data-replace-team-package-access-form]'
    )
    .forEach((form) => {
      form.addEventListener('submit', async (event) => {
        event.preventDefault();

        const teamSlug = form.getAttribute(
          'data-replace-team-package-access-form'
        );
        if (!teamSlug) {
          return;
        }

        const formData = new FormData(form);
        const packageSelection =
          formData.get('package_key')?.toString().trim() || '';
        const packageTarget = decodePackageSelection(packageSelection);

        if (!packageTarget) {
          await loadAndRender(container, slug, {
            error: 'Select an organization package to manage access.',
          });
          return;
        }

        const permissions = formData
          .getAll('permissions')
          .map((entry) => entry.toString().trim())
          .filter(Boolean);

        if (permissions.length === 0) {
          await loadAndRender(container, slug, {
            error: 'Select at least one delegated package permission.',
          });
          return;
        }

        const submitButton = getSubmitButton(form);
        if (!submitButton) {
          return;
        }

        submitButton.disabled = true;
        submitButton.textContent = 'Saving…';

        try {
          await replaceTeamPackageAccess(
            slug,
            teamSlug,
            packageTarget.ecosystem,
            packageTarget.name,
            {
              permissions,
            }
          );

          await loadAndRender(container, slug, {
            notice: `Saved package access for ${packageTarget.name} in ${teamSlug}.`,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, slug, {
            error: toErrorMessage(
              caughtError,
              'Failed to update package access.'
            ),
          });
        }
      });
    });

  container
    .querySelectorAll<HTMLButtonElement>('[data-remove-team-member]')
    .forEach((button) => {
      button.addEventListener('click', async () => {
        const teamSlug = button.getAttribute('data-team-slug');
        const username = button.getAttribute('data-username');
        if (!teamSlug || !username) {
          return;
        }

        button.disabled = true;
        button.textContent = 'Removing…';

        try {
          await removeTeamMember(slug, teamSlug, username);
          await loadAndRender(container, slug, {
            notice: `Removed @${username} from ${teamSlug}.`,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, slug, {
            error: toErrorMessage(caughtError, 'Failed to remove team member.'),
          });
        }
      });
    });

  container
    .querySelectorAll<HTMLButtonElement>('[data-remove-team-package-access]')
    .forEach((button) => {
      button.addEventListener('click', async () => {
        const teamSlug = button.getAttribute('data-team-slug');
        const ecosystem = button.getAttribute('data-package-ecosystem');
        const packageName = button.getAttribute('data-package-name');
        if (!teamSlug || !ecosystem || !packageName) {
          return;
        }

        button.disabled = true;
        button.textContent = 'Revoking…';

        try {
          await removeTeamPackageAccess(slug, teamSlug, ecosystem, packageName);
          await loadAndRender(container, slug, {
            notice: `Revoked package access for ${packageName} in ${teamSlug}.`,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, slug, {
            error: toErrorMessage(
              caughtError,
              'Failed to revoke package access.'
            ),
          });
        }
      });
    });

  const namespaceForm = container.querySelector<HTMLFormElement>(
    '#org-namespace-form'
  );
  namespaceForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const orgId = org.id?.trim();
    if (!orgId) {
      await loadAndRender(container, slug, {
        error:
          'Failed to create the namespace claim because the organization id is unavailable.',
      });
      return;
    }

    const formData = new FormData(namespaceForm);
    const ecosystem =
      formData.get('ecosystem')?.toString().trim().toLowerCase() || '';
    const namespace = formData.get('namespace')?.toString().trim() || '';

    if (!ecosystem) {
      await loadAndRender(container, slug, {
        error: 'Select an ecosystem before creating a namespace claim.',
      });
      return;
    }

    if (!namespace) {
      await loadAndRender(container, slug, {
        error: 'Enter the namespace you want this organization to claim.',
      });
      return;
    }

    const submitButton = getSubmitButton(namespaceForm);
    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Claiming…';

    try {
      await createNamespaceClaim({
        ecosystem,
        namespace,
        ownerOrgId: orgId,
      });

      await loadAndRender(container, slug, {
        notice: `Created the ${ecosystemLabel(ecosystem)} namespace claim ${namespace}.`,
      });
    } catch (caughtError: unknown) {
      await loadAndRender(container, slug, {
        error: toErrorMessage(
          caughtError,
          'Failed to create the namespace claim.'
        ),
      });
    }
  });

  const repositoryCreateForm = container.querySelector<HTMLFormElement>(
    '#org-repository-form'
  );
  repositoryCreateForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const orgId = org.id?.trim();
    if (!orgId) {
      await loadAndRender(container, slug, {
        error:
          'Failed to create the repository because the organization id is unavailable.',
      });
      return;
    }

    const formData = new FormData(repositoryCreateForm);
    const name = formData.get('name')?.toString().trim() || '';
    const repositorySlug = formData.get('slug')?.toString().trim() || '';
    const kind = formData.get('kind')?.toString().trim() || 'public';
    const visibility =
      formData.get('visibility')?.toString().trim() || 'public';

    if (!name) {
      await loadAndRender(container, slug, {
        error: 'Enter a repository name before creating it.',
      });
      return;
    }

    if (!repositorySlug) {
      await loadAndRender(container, slug, {
        error: 'Enter a repository slug before creating it.',
      });
      return;
    }

    const submitButton = getSubmitButton(repositoryCreateForm);
    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Creating…';

    try {
      await createRepository({
        name,
        slug: repositorySlug,
        kind,
        visibility,
        description: normalizeFormOptionalText(formData.get('description')),
        upstreamUrl: normalizeFormOptionalText(formData.get('upstream_url')),
        ownerOrgId: orgId,
      });

      await loadAndRender(container, slug, {
        notice: `Created the repository ${repositorySlug}.`,
      });
    } catch (caughtError: unknown) {
      await loadAndRender(container, slug, {
        error: toErrorMessage(caughtError, 'Failed to create the repository.'),
      });
    }
  });

  container
    .querySelectorAll<HTMLFormElement>('[data-update-repository-form]')
    .forEach((form) => {
      form.addEventListener('submit', async (event) => {
        event.preventDefault();

        const repositorySlug = form.getAttribute('data-update-repository-form');
        if (!repositorySlug) {
          return;
        }

        const repository = repositories.find(
          (candidate) => candidate.slug === repositorySlug
        );
        if (!repository) {
          await loadAndRender(container, slug, {
            error: `Failed to find the repository ${repositorySlug}.`,
          });
          return;
        }

        const formData = new FormData(form);
        const nextDescription =
          formData.get('description')?.toString().trim() ?? '';
        const nextVisibility =
          formData.get('visibility')?.toString().trim() || 'public';
        const nextUpstreamUrl =
          formData.get('upstream_url')?.toString().trim() ?? '';
        const currentDescription = repository.description?.trim() || '';
        const currentVisibility = repository.visibility?.trim() || 'public';
        const currentUpstreamUrl = repository.upstream_url?.trim() || '';

        if (
          nextDescription === currentDescription &&
          nextVisibility === currentVisibility &&
          nextUpstreamUrl === currentUpstreamUrl
        ) {
          await loadAndRender(container, slug, {
            notice: `${repository.name || repositorySlug} is already up to date.`,
          });
          return;
        }

        const submitButton = getSubmitButton(form);
        if (!submitButton) {
          return;
        }

        submitButton.disabled = true;
        submitButton.textContent = 'Saving…';

        try {
          await updateRepository(repositorySlug, {
            description: nextDescription,
            visibility: nextVisibility,
            upstreamUrl: nextUpstreamUrl,
          });

          await loadAndRender(container, slug, {
            notice: `Updated the repository ${repository.name || repositorySlug}.`,
          });
        } catch (caughtError: unknown) {
          await loadAndRender(container, slug, {
            error: toErrorMessage(
              caughtError,
              'Failed to update the repository.'
            ),
          });
        }
      });
    });

  const packageTransferForm = container.querySelector<HTMLFormElement>(
    '#org-package-transfer-form'
  );
  packageTransferForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const formData = new FormData(packageTransferForm);
    const packageSelection =
      formData.get('package_key')?.toString().trim() || '';
    const packageTarget = decodePackageSelection(packageSelection);
    const targetOrgSlug =
      formData.get('target_org_slug')?.toString().trim() || '';
    const confirmBox = packageTransferForm.querySelector<HTMLInputElement>(
      '#org-package-transfer-confirm'
    );

    if (!packageTarget) {
      await loadAndRender(container, slug, {
        error: 'Select an organization package to transfer.',
      });
      return;
    }

    if (!targetOrgSlug) {
      await loadAndRender(container, slug, {
        error: 'Select the organization that should receive this package.',
      });
      return;
    }

    if (targetOrgSlug.trim().toLowerCase() === slug.trim().toLowerCase()) {
      await loadAndRender(container, slug, {
        error:
          'Select a different organization before transferring package ownership.',
      });
      return;
    }

    if (!confirmBox?.checked) {
      await loadAndRender(container, slug, {
        error:
          'Please confirm that you understand this transfer is immediate and revokes existing team grants.',
      });
      return;
    }

    const submitButton = getSubmitButton(packageTransferForm);
    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Transferring…';

    try {
      const result = await transferPackageOwnership(
        packageTarget.ecosystem,
        packageTarget.name,
        {
          targetOrgSlug,
        }
      );
      const targetLabel =
        result.owner?.name || result.owner?.slug || targetOrgSlug;

      await loadAndRender(container, slug, {
        notice: `Transferred ${packageTarget.name} to ${targetLabel}.`,
      });
    } catch (caughtError: unknown) {
      await loadAndRender(container, slug, {
        error: toErrorMessage(
          caughtError,
          'Failed to transfer package ownership.'
        ),
      });
    }
  });
}

async function loadTeamMembers(
  slug: string,
  teams: Team[]
): Promise<Record<string, TeamMemberState>> {
  const teamEntries = await Promise.all(
    teams.filter(hasTeamSlug).map(async (team) => {
      try {
        const data = await listTeamMembers(slug, team.slug);
        return [
          team.slug,
          {
            members: data.members || [],
            load_error: null,
          },
        ] as const;
      } catch (caughtError: unknown) {
        return [
          team.slug,
          {
            members: [],
            load_error: toErrorMessage(
              caughtError,
              `Failed to load members for ${team.name || team.slug}.`
            ),
          },
        ] as const;
      }
    })
  );

  return Object.fromEntries(teamEntries);
}

async function loadTeamPackageAccess(
  slug: string,
  teams: Team[]
): Promise<Record<string, TeamPackageAccessState>> {
  const teamEntries = await Promise.all(
    teams.filter(hasTeamSlug).map(async (team) => {
      try {
        const data: TeamPackageAccessListResponse = await listTeamPackageAccess(
          slug,
          team.slug
        );
        return [
          team.slug,
          {
            grants: data.package_access || [],
            load_error: null,
          },
        ] as const;
      } catch (caughtError: unknown) {
        return [
          team.slug,
          {
            grants: [],
            load_error: toErrorMessage(
              caughtError,
              `Failed to load package access for ${team.name || team.slug}.`
            ),
          },
        ] as const;
      }
    })
  );

  return Object.fromEntries(teamEntries);
}

async function loadRepositoryPackages(
  repositories: OrgRepositorySummary[]
): Promise<Record<string, RepositoryPackageState>> {
  const repositoryEntries = await Promise.all(
    repositories.filter(hasRepositorySlug).map(async (repository) => {
      try {
        const data: RepositoryPackageListResponse =
          await listRepositoryPackages(repository.slug, {
            perPage: 100,
          });

        return [
          repository.slug,
          {
            packages: data.packages || [],
            load_error: data.load_error || null,
          },
        ] as const;
      } catch (caughtError: unknown) {
        return [
          repository.slug,
          {
            packages: [],
            load_error: toErrorMessage(
              caughtError,
              `Failed to load packages for ${repository.name || repository.slug}.`
            ),
          },
        ] as const;
      }
    })
  );

  return Object.fromEntries(repositoryEntries);
}

function hasRepositorySlug(
  repository: OrgRepositorySummary
): repository is OrgRepositorySummary & { slug: string } {
  return (
    typeof repository.slug === 'string' && repository.slug.trim().length > 0
  );
}

function renderTeamCard(
  team: Team,
  {
    canAdminister,
    packages,
    packagesError,
    teamMemberState,
    teamPackageAccessState,
  }: {
    canAdminister: boolean;
    packages: OrgPackageSummary[];
    packagesError: string | null;
    teamMemberState: TeamMemberState;
    teamPackageAccessState: TeamPackageAccessState;
  }
): string {
  const members = teamMemberState.members || [];
  const memberCount = members.length;

  return `
    <div class="card">
      <div class="org-section-header">
        <div class="token-row__main">
          <div class="token-row__title">${escapeHtml(team.name || team.slug || 'Team')}</div>
          <div class="token-row__meta">
            <span>@${escapeHtml(team.slug || 'no-slug')}</span>
            <span>created ${escapeHtml(formatDate(team.created_at))}</span>
          </div>
          ${
            canAdminister
              ? `
                <div class="token-row__scopes">
                  <span class="badge badge-ecosystem">${escapeHtml(String(memberCount))} members</span>
                </div>
              `
              : ''
          }
          ${team.description ? `<p class="settings-copy">${escapeHtml(team.description)}</p>` : '<p class="settings-copy">No team description yet.</p>'}
        </div>
        ${
          canAdminister
            ? `
              <div class="token-row__actions">
                <button class="btn btn-danger btn-sm" data-delete-team="${escapeHtml(team.slug || '')}" type="button">Delete team</button>
              </div>
            `
            : ''
        }
      </div>

      ${
        canAdminister
          ? `
            <div class="settings-subsection">
              <div class="grid gap-6 xl:grid-cols-2">
                <form data-update-team-form="${escapeHtml(team.slug || '')}" class="settings-section">
                  <h3>Team details</h3>
                  <p class="settings-copy">Update the team name or description here. The slug remains stable for API integrations.</p>
                  <div class="form-group">
                    <label for="team-name-${escapeHtml(team.slug || 'team')}">Team name</label>
                    <input id="team-name-${escapeHtml(team.slug || 'team')}" name="name" class="form-input" value="${escapeHtml(team.name || '')}" required />
                  </div>
                  <div class="form-group">
                    <label for="team-description-${escapeHtml(team.slug || 'team')}">Description</label>
                    <textarea id="team-description-${escapeHtml(team.slug || 'team')}" name="description" class="form-input" rows="3">${escapeHtml(team.description || '')}</textarea>
                  </div>
                  <button type="submit" class="btn btn-secondary">Save changes</button>
                </form>

                <div class="settings-section">
                  <div class="org-section-header">
                    <div>
                      <h3>Team members</h3>
                      <p class="settings-copy">Members added here must already belong to the parent organization.</p>
                    </div>
                  </div>

                  ${renderTeamMembers(team, members, teamMemberState.load_error)}

                  <form data-add-team-member-form="${escapeHtml(team.slug || '')}">
                    <div class="form-group">
                      <label for="team-member-${escapeHtml(team.slug || 'team')}">Add organization member</label>
                      <input id="team-member-${escapeHtml(team.slug || 'team')}" name="username" class="form-input" placeholder="alice" required />
                    </div>
                    <button type="submit" class="btn btn-primary">Add team member</button>
                  </form>
                </div>
              </div>

              <div class="settings-section mt-6">
                <div class="org-section-header">
                  <div>
                    <h3>Package access</h3>
                    <p class="settings-copy">Delegate package-scoped responsibilities to this team for organization-owned packages listed in this workspace. Saving replaces the full permission set for the selected package.</p>
                  </div>
                </div>

                ${renderTeamPackageAccess(
                  team,
                  teamPackageAccessState.grants,
                  teamPackageAccessState.load_error
                )}

                ${renderTeamPackageAccessForm(team, packages, packagesError)}
              </div>
            </div>
          `
          : ''
      }
    </div>
  `;
}

function renderTeamMembers(
  team: Team,
  members: TeamMember[],
  loadError: string | null
): string {
  if (loadError) {
    return `<div class="alert alert-error">${escapeHtml(loadError)}</div>`;
  }

  if (members.length === 0) {
    return `<p class="settings-copy">No members have been added to ${escapeHtml(team.name || team.slug || 'this team')} yet.</p>`;
  }

  return `
    <div class="token-list">
      ${members
        .map(
          (member) => `
            <div class="token-row">
              <div class="token-row__main">
                <div class="token-row__title">${escapeHtml(member.display_name || member.username || 'Unknown member')}</div>
                <div class="token-row__meta">
                  <span>@${escapeHtml(member.username || 'unknown')}</span>
                  <span>added ${escapeHtml(formatDate(member.added_at))}</span>
                </div>
              </div>
              <div class="token-row__actions">
                <button
                  class="btn btn-secondary btn-sm"
                  data-remove-team-member
                  data-team-slug="${escapeHtml(team.slug || '')}"
                  data-username="${escapeHtml(member.username || '')}"
                  type="button"
                >
                  Remove
                </button>
              </div>
            </div>
          `
        )
        .join('')}
    </div>
  `;
}

function renderAuditLogRow(
  log: OrgAuditLog,
  activeActorUserId: string
): string {
  const title = formatAuditActionLabel(log.action || 'activity');
  const actor = formatAuditActor(log);
  const target = formatAuditTarget(log);
  const summary = formatAuditSummary(log);
  const actorUserId = normalizeAuditActorUserId(log.actor_user_id);
  const actorUsername = normalizeAuditActorUsername(log.actor_username);
  const meta = [
    actor ? `by ${actor}` : null,
    target,
    log.occurred_at ? formatDate(log.occurred_at) : null,
  ].filter((value): value is string => Boolean(value));
  const actorFilterButton =
    actorUserId && actorUserId !== activeActorUserId
      ? `
          <div class="token-row__actions">
            <button
              class="btn btn-secondary btn-sm"
              type="button"
              data-audit-actor-user-id="${escapeHtml(actorUserId)}"
              data-audit-actor-username="${escapeHtml(actorUsername)}"
              title="Filter this activity log to ${escapeHtml(actor || formatAuditActorQueryLabel(actorUsername))}"
            >
              Only this actor
            </button>
          </div>
        `
      : '';

  return `
    <div class="token-row">
      <div class="token-row__main">
        <div class="token-row__title">${escapeHtml(title)}</div>
        ${
          meta.length > 0
            ? `<div class="token-row__meta">${meta
                .map((item) => `<span>${escapeHtml(item)}</span>`)
                .join('')}</div>`
            : ''
        }
        ${summary ? `<p class="settings-copy">${escapeHtml(summary)}</p>` : ''}
      </div>
      ${actorFilterButton}
    </div>
  `;
}

function renderNamespaceClaims(
  namespaceClaims: NamespaceClaim[],
  namespaceError: string | null,
  canAdminister: boolean
): string {
  if (namespaceError) {
    return `<div class="alert alert-error">${escapeHtml(namespaceError)}</div>`;
  }

  if (namespaceClaims.length === 0) {
    return `<div class="empty-state"><h3>No namespace claims yet</h3><p>${escapeHtml(
      canAdminister
        ? 'Claim a namespace below to reserve prefixes such as @acme, com.acme, or an ecosystem vendor string for this organization.'
        : 'This organization has not claimed any ecosystem namespaces yet.'
    )}</p></div>`;
  }

  return `
    <div class="token-list">
      ${[...namespaceClaims]
        .sort((left, right) => {
          const leftKey = `${left.ecosystem || ''}:${left.namespace || ''}`;
          const rightKey = `${right.ecosystem || ''}:${right.namespace || ''}`;
          return leftKey.localeCompare(rightKey);
        })
        .map(
          (claim) => `
            <div class="token-row">
              <div class="token-row__main">
                <div class="token-row__title">${escapeHtml(claim.namespace || 'Unnamed claim')}</div>
                <div class="token-row__meta">
                  <span>${escapeHtml(ecosystemLabel(claim.ecosystem))}</span>
                  ${claim.created_at ? `<span>created ${escapeHtml(formatDate(claim.created_at))}</span>` : ''}
                </div>
              </div>
              <div class="token-row__actions">
                ${
                  claim.is_verified
                    ? '<span class="badge badge-verified">Verified</span>'
                    : '<span class="badge badge-ecosystem">Pending verification</span>'
                }
              </div>
            </div>
          `
        )
        .join('')}
    </div>
  `;
}

function renderNamespaceClaimForm(): string {
  return `
    <form id="org-namespace-form" class="settings-subsection">
      <h3>Claim a namespace</h3>
      <p class="settings-copy">Claims are created immediately. Verification, transfer, and revocation workflows are future slices, so choose the namespace carefully.</p>

      <div class="grid gap-4 xl:grid-cols-2">
        <div class="form-group">
          <label for="org-namespace-ecosystem">Ecosystem</label>
          <select id="org-namespace-ecosystem" name="ecosystem" class="form-input" required>
            ${renderNamespaceEcosystemOptions(DEFAULT_NAMESPACE_ECOSYSTEM)}
          </select>
        </div>

        <div class="form-group">
          <label for="org-namespace-value">Namespace</label>
          <input id="org-namespace-value" name="namespace" class="form-input" placeholder="@acme, acme, com.acme, ghcr.io/acme" required />
        </div>
      </div>

      <button type="submit" class="btn btn-primary">Create namespace claim</button>
    </form>
  `;
}

function renderOrgRepositories(
  repositories: OrgRepositorySummary[],
  repositoryPackagesBySlug: Record<string, RepositoryPackageState>,
  repositoriesError: string | null,
  canAdminister: boolean
): string {
  if (repositoriesError) {
    return `<div class="alert alert-error">${escapeHtml(repositoriesError)}</div>`;
  }

  if (repositories.length === 0) {
    return `<div class="empty-state"><h3>No repositories yet</h3><p>${escapeHtml(
      canAdminister
        ? 'Create the first organization repository below to separate public, internal, staging, or mirrored package sources.'
        : 'This organization has not exposed any public repositories yet.'
    )}</p></div>`;
  }

  return `
    <div class="settings-section">
      ${[...repositories]
        .sort((left, right) => {
          const leftKey = (left.name || left.slug || '').toLowerCase();
          const rightKey = (right.name || right.slug || '').toLowerCase();
          return leftKey.localeCompare(rightKey);
        })
        .map((repository) =>
          renderOrgRepositoryCard(
            repository,
            repository.slug
              ? repositoryPackagesBySlug[repository.slug] || null
              : null,
            canAdminister
          )
        )
        .join('')}
    </div>
  `;
}

function renderOrgRepositoryCard(
  repository: OrgRepositorySummary,
  repositoryPackageState: RepositoryPackageState | null,
  canAdminister: boolean
): string {
  const repositorySlug = repository.slug || '';
  const repositoryName = repository.name || repositorySlug || 'Repository';

  return `
    <div class="settings-subsection">
      <div class="org-section-header">
        <div class="token-row__main">
          <div class="token-row__title">${
            repositorySlug
              ? `<a href="/repositories/${encodeURIComponent(repositorySlug)}">${escapeHtml(repositoryName)}</a>`
              : escapeHtml(repositoryName)
          }</div>
          <div class="token-row__meta">
            <span>@${escapeHtml(repositorySlug || 'no-slug')}</span>
            <span>${escapeHtml(formatRepositoryKindLabel(repository.kind))}</span>
            <span>${escapeHtml(formatRepositoryVisibilityLabel(repository.visibility))}</span>
            <span>${escapeHtml(formatNumber(repository.package_count))} packages</span>
            ${repository.created_at ? `<span>created ${escapeHtml(formatDate(repository.created_at))}</span>` : ''}
          </div>
          ${repository.description ? `<p class="settings-copy">${escapeHtml(repository.description)}</p>` : ''}
          ${repository.upstream_url ? `<p class="settings-copy"><a href="${escapeHtml(repository.upstream_url)}" target="_blank" rel="noopener noreferrer">${escapeHtml(repository.upstream_url)}</a></p>` : ''}
        </div>
      </div>

      ${renderRepositoryPackageList(repository, repositoryPackageState)}
      ${canAdminister ? renderOrgRepositoryUpdateForm(repository) : ''}
    </div>
  `;
}

function renderRepositoryPackageList(
  repository: OrgRepositorySummary,
  repositoryPackageState: RepositoryPackageState | null
): string {
  const repositoryPackages = repositoryPackageState?.packages || [];
  const coverageLabel = formatRepositoryPackageCoverageLabel(
    repositoryPackages.length,
    repository.package_count
  );

  if (repositoryPackageState?.load_error) {
    return `
      <div class="settings-section">
        <div class="org-section-header">
          <div>
            <h3>Visible packages</h3>
            <p class="settings-copy">${escapeHtml(coverageLabel)}</p>
          </div>
        </div>
        <div class="alert alert-error">${escapeHtml(repositoryPackageState.load_error)}</div>
      </div>
    `;
  }

  if (repositoryPackages.length === 0) {
    return `
      <div class="settings-section">
        <div class="org-section-header">
          <div>
            <h3>Visible packages</h3>
            <p class="settings-copy">${escapeHtml(coverageLabel)}</p>
          </div>
        </div>
      </div>
    `;
  }

  return `
    <div class="settings-section">
      <div class="org-section-header">
        <div>
          <h3>Visible packages</h3>
          <p class="settings-copy">${escapeHtml(coverageLabel)}</p>
        </div>
      </div>

      <div class="token-list">
        ${repositoryPackages
          .map(
            (pkg) => `
              <div class="token-row">
                <div class="token-row__main">
                  <div class="token-row__title">
                    <a href="/packages/${encodeURIComponent(pkg.ecosystem || 'unknown')}/${encodeURIComponent(pkg.name || '')}">${escapeHtml(pkg.name || 'Unnamed package')}</a>
                  </div>
                  <div class="token-row__meta">
                    <span>${escapeHtml(pkg.ecosystem || 'unknown')}</span>
                    <span>${escapeHtml(formatRepositoryVisibilityLabel(pkg.visibility))}</span>
                    <span>${escapeHtml(formatNumber(pkg.download_count))} downloads</span>
                    ${pkg.created_at ? `<span>created ${escapeHtml(formatDate(pkg.created_at))}</span>` : ''}
                  </div>
                  ${pkg.description ? `<p class="settings-copy">${escapeHtml(pkg.description)}</p>` : ''}
                </div>
              </div>
            `
          )
          .join('')}
      </div>
    </div>
  `;
}

function renderOrgRepositoryCreateForm(): string {
  return `
    <form id="org-repository-form" class="settings-subsection">
      <h3>Create a repository</h3>
      <p class="settings-copy">Repositories define visibility boundaries and source-of-truth lanes for package publication, staging, and mirroring.</p>

      <div class="grid gap-4 xl:grid-cols-2">
        <div class="form-group">
          <label for="org-repository-name">Repository name</label>
          <input id="org-repository-name" name="name" class="form-input" placeholder="Acme Public" required />
        </div>
        <div class="form-group">
          <label for="org-repository-slug">Repository slug</label>
          <input id="org-repository-slug" name="slug" class="form-input" placeholder="acme-public" required />
        </div>
      </div>

      <div class="grid gap-4 xl:grid-cols-2">
        <div class="form-group">
          <label for="org-repository-kind">Repository kind</label>
          <select id="org-repository-kind" name="kind" class="form-input" required>
            ${renderRepositoryKindOptions('public')}
          </select>
        </div>
        <div class="form-group">
          <label for="org-repository-visibility">Visibility</label>
          <select id="org-repository-visibility" name="visibility" class="form-input" required>
            ${renderRepositoryVisibilityOptions('public')}
          </select>
        </div>
      </div>

      <div class="form-group">
        <label for="org-repository-upstream">Upstream URL</label>
        <input id="org-repository-upstream" name="upstream_url" class="form-input" type="url" placeholder="https://registry.npmjs.org" />
      </div>

      <div class="form-group">
        <label for="org-repository-description">Description</label>
        <textarea id="org-repository-description" name="description" class="form-input" rows="3" placeholder="Public release channel for Acme packages."></textarea>
      </div>

      <button type="submit" class="btn btn-primary">Create repository</button>
    </form>
  `;
}

function renderOrgRepositoryUpdateForm(
  repository: OrgRepositorySummary
): string {
  const repositorySlug = repository.slug || '';

  return `
    <form data-update-repository-form="${escapeHtml(repositorySlug)}" class="settings-section">
      <h3>Repository settings</h3>
      <p class="settings-copy">Repository kind and slug stay stable; visibility, upstream URL, and description can evolve over time.</p>

      <div class="grid gap-4 xl:grid-cols-2">
        <div class="form-group">
          <label for="repository-kind-${escapeHtml(repositorySlug || 'repository')}">Repository kind</label>
          <input id="repository-kind-${escapeHtml(repositorySlug || 'repository')}" class="form-input" value="${escapeHtml(formatRepositoryKindLabel(repository.kind))}" disabled />
        </div>
        <div class="form-group">
          <label for="repository-visibility-${escapeHtml(repositorySlug || 'repository')}">Visibility</label>
          <select id="repository-visibility-${escapeHtml(repositorySlug || 'repository')}" name="visibility" class="form-input">
            ${renderRepositoryVisibilityOptions(repository.visibility || 'public')}
          </select>
        </div>
      </div>

      <div class="form-group">
        <label for="repository-upstream-${escapeHtml(repositorySlug || 'repository')}">Upstream URL</label>
        <input id="repository-upstream-${escapeHtml(repositorySlug || 'repository')}" name="upstream_url" class="form-input" type="url" placeholder="https://registry.npmjs.org" value="${escapeHtml(repository.upstream_url || '')}" />
      </div>

      <div class="form-group">
        <label for="repository-description-${escapeHtml(repositorySlug || 'repository')}">Description</label>
        <textarea id="repository-description-${escapeHtml(repositorySlug || 'repository')}" name="description" class="form-input" rows="3">${escapeHtml(repository.description || '')}</textarea>
      </div>

      <button type="submit" class="btn btn-secondary">Save repository</button>
    </form>
  `;
}

function renderRepositoryKindOptions(selectedValue: string): string {
  return REPOSITORY_KIND_OPTIONS.map(
    (option) => `
      <option value="${option.value}" ${option.value === selectedValue ? 'selected' : ''}>
        ${escapeHtml(option.label)}
      </option>
    `
  ).join('');
}

function renderRepositoryVisibilityOptions(selectedValue: string): string {
  return REPOSITORY_VISIBILITY_OPTIONS.map(
    (option) => `
      <option value="${option.value}" ${option.value === selectedValue ? 'selected' : ''}>
        ${escapeHtml(option.label)}
      </option>
    `
  ).join('');
}

function renderNamespaceEcosystemOptions(selectedValue: string): string {
  return ECOSYSTEMS.map(
    (ecosystem) => `
      <option value="${ecosystem.id}" ${ecosystem.id === selectedValue ? 'selected' : ''}>
        ${escapeHtml(ecosystem.label)}
      </option>
    `
  ).join('');
}

function renderTeamPackageAccess(
  team: Team,
  grants: TeamPackageAccessGrant[],
  loadError: string | null
): string {
  if (loadError) {
    return `<div class="alert alert-error">${escapeHtml(loadError)}</div>`;
  }

  if (grants.length === 0) {
    return `<p class="settings-copy">No package grants have been assigned to ${escapeHtml(team.name || team.slug || 'this team')} yet.</p>`;
  }

  return `
    <div class="token-list">
      ${grants
        .map(
          (grant) => `
            <div class="token-row">
              <div class="token-row__main">
                <div class="token-row__title">
                  <a href="/packages/${encodeURIComponent(grant.ecosystem || 'unknown')}/${encodeURIComponent(grant.name || '')}">${escapeHtml(grant.name || 'Unnamed package')}</a>
                </div>
                <div class="token-row__meta">
                  <span>${escapeHtml(grant.ecosystem || 'unknown')}</span>
                  <span>granted ${escapeHtml(formatDate(grant.granted_at))}</span>
                </div>
                ${renderPermissionBadges(grant.permissions || [])}
              </div>
              <div class="token-row__actions">
                <button
                  class="btn btn-secondary btn-sm"
                  data-remove-team-package-access
                  data-team-slug="${escapeHtml(team.slug || '')}"
                  data-package-ecosystem="${escapeHtml(grant.ecosystem || '')}"
                  data-package-name="${escapeHtml(grant.name || '')}"
                  type="button"
                >
                  Revoke
                </button>
              </div>
            </div>
          `
        )
        .join('')}
    </div>
  `;
}

function renderTeamPackageAccessForm(
  team: Team,
  packages: OrgPackageSummary[],
  packagesError: string | null
): string {
  const formDisabled = Boolean(packagesError) || packages.length === 0;

  return `
    <form data-replace-team-package-access-form="${escapeHtml(team.slug || '')}" class="settings-subsection">
      <div class="form-group">
        <label for="team-package-${escapeHtml(team.slug || 'team')}">Organization package</label>
        ${
          packagesError
            ? `<div class="alert alert-error">${escapeHtml(packagesError)}</div>`
            : packages.length === 0
              ? '<p class="settings-copy">Create or transfer an organization-owned package before delegating team access.</p>'
              : `
                  <select id="team-package-${escapeHtml(team.slug || 'team')}" name="package_key" class="form-input" required>
                    <option value="">Select a package</option>
                    ${renderPackageSelectionOptions(packages)}
                  </select>
                `
        }
      </div>

      <fieldset class="form-group">
        <legend>Permissions</legend>
        <p class="settings-copy">
          Under the current backend model, any package grant also unlocks non-public reads for that package.
          <strong>Security review</strong> remains reserved for future workflows.
        </p>
        <div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          ${renderTeamPermissionOptions(formDisabled)}
        </div>
      </fieldset>

      <button type="submit" class="btn btn-primary"${formDisabled ? ' disabled' : ''}>Save package access</button>
    </form>
  `;
}

function renderOrgPackageTransferForm(
  currentOrgSlug: string,
  packages: OrgPackageSummary[],
  packagesError: string | null,
  transferTargets: OrganizationMembership[]
): string {
  const transferablePackages = selectTransferablePackages(packages);

  return `
    <div class="settings-subsection">
      <h3>Transfer package ownership</h3>
      <div class="alert alert-warning" style="margin-bottom:12px;">
        This transfer is immediate and revokes existing team grants on the package.
      </div>
      <p class="settings-copy">
        Move an organization-owned package from @${escapeHtml(currentOrgSlug)} into another organization you already administer.
      </p>
      ${renderOrgPackageTransferFormBody(
        transferablePackages,
        packagesError,
        transferTargets
      )}
    </div>
  `;
}

function renderOrgPackageTransferFormBody(
  transferablePackages: OrgPackageSummary[],
  packagesError: string | null,
  transferTargets: OrganizationMembership[]
): string {
  if (packagesError) {
    return `<p class="settings-copy">Packages must load successfully before you can transfer one to another organization.</p>`;
  }

  if (transferablePackages.length === 0) {
    return `<p class="settings-copy">No visible packages are currently transferable with this credential.</p>`;
  }

  if (transferTargets.length === 0) {
    return `<p class="settings-copy">You do not currently administer another organization that can receive one of these packages.</p>`;
  }

  return `
    <form id="org-package-transfer-form">
      <div class="grid gap-4 xl:grid-cols-2">
        <div class="form-group">
          <label for="org-package-transfer-package">Organization package</label>
          <select id="org-package-transfer-package" name="package_key" class="form-input" required>
            <option value="">Select a package</option>
            ${renderPackageSelectionOptions(transferablePackages)}
          </select>
        </div>
        <div class="form-group">
          <label for="org-package-transfer-target">Target organization</label>
          <select id="org-package-transfer-target" name="target_org_slug" class="form-input" required>
            <option value="">Select an organization</option>
            ${renderPackageTransferTargetOptions(transferTargets)}
          </select>
        </div>
      </div>
      <div class="form-group" style="margin-bottom:12px;">
        <label class="flex items-start gap-2">
          <input type="checkbox" id="org-package-transfer-confirm" name="confirm" required />
          <span>I understand this package transfer is immediate and existing team grants will be removed.</span>
        </label>
      </div>
      <button type="submit" class="btn btn-danger">Transfer package</button>
    </form>
  `;
}

function renderPackageTransferTargetOptions(
  organizations: OrganizationMembership[]
): string {
  return organizations
    .map(
      (organization) => `
        <option value="${escapeHtml(organization.slug || '')}">
          ${escapeHtml(organization.name || organization.slug || 'Unnamed organization')}
        </option>
      `
    )
    .join('');
}

function renderTeamPermissionOptions(disabled: boolean): string {
  return TEAM_PERMISSION_OPTIONS.map(
    (permission) => `
      <label class="rounded-lg border border-neutral-200 p-3 text-sm ${
        disabled ? 'opacity-60' : ''
      }">
        <span class="flex items-start gap-3">
          <input
            type="checkbox"
            name="permissions"
            value="${permission.value}"
            ${disabled ? 'disabled' : ''}
          />
          <span>
            <span class="block font-medium">${permission.label}</span>
            <span class="mt-1 block text-muted">${permission.description}</span>
          </span>
        </span>
      </label>
    `
  ).join('');
}

function renderAuditActionOptions(selectedValue: string): string {
  return ORG_AUDIT_ACTION_VALUES.map(
    (action) => `
      <option value="${action}" ${action === selectedValue ? 'selected' : ''}>
        ${escapeHtml(formatAuditActionLabel(action))}
      </option>
    `
  ).join('');
}

function formatAuditFilterSummary(
  auditPage: number,
  auditAction: string,
  auditActorUserId: string,
  auditActorUsername: string
): string {
  const prefix = `Showing page ${auditPage} with up to ${ORG_AUDIT_PAGE_SIZE} events`;

  if (auditAction && auditActorUserId) {
    return `${prefix} filtered to ${formatAuditActionLabel(auditAction).toLowerCase()} activity by ${formatAuditActorQueryLabel(auditActorUsername)}.`;
  }

  if (auditAction) {
    return `${prefix} filtered to ${formatAuditActionLabel(auditAction).toLowerCase()}.`;
  }

  if (auditActorUserId) {
    return `${prefix} filtered to activity by ${formatAuditActorQueryLabel(auditActorUsername)}.`;
  }

  return `${prefix}.`;
}

function formatAuditActionLabel(action: string): string {
  switch (action) {
    case 'org_create':
      return 'Organization created';
    case 'org_update':
      return 'Organization updated';
    case 'namespace_claim_create':
      return 'Namespace claim created';
    case 'org_member_add':
      return 'Member added';
    case 'org_role_change':
      return 'Member role updated';
    case 'org_member_remove':
      return 'Member removed';
    case 'org_ownership_transfer':
      return 'Ownership transferred';
    case 'org_invitation_create':
      return 'Invitation sent';
    case 'org_invitation_revoke':
      return 'Invitation revoked';
    case 'org_invitation_accept':
      return 'Invitation accepted';
    case 'org_invitation_decline':
      return 'Invitation declined';
    case 'team_create':
      return 'Team created';
    case 'team_update':
      return 'Team updated';
    case 'team_delete':
      return 'Team deleted';
    case 'team_member_add':
      return 'Team member added';
    case 'team_member_remove':
      return 'Team member removed';
    case 'team_package_access_update':
      return 'Package access updated';
    default:
      return formatIdentifierLabel(action || 'activity');
  }
}

function formatAuditActor(log: OrgAuditLog): string | null {
  const displayName = log.actor_display_name?.trim();
  const username = log.actor_username?.trim();

  if (displayName && username && displayName !== username) {
    return `${displayName} (@${username})`;
  }
  if (displayName) {
    return displayName;
  }
  if (username) {
    return `@${username}`;
  }

  return null;
}

function formatAuditTarget(log: OrgAuditLog): string | null {
  const metadata = log.metadata;
  const username =
    log.target_username?.trim() ||
    getAuditMetadataString(metadata, 'username') ||
    getAuditMetadataString(metadata, 'invited_username') ||
    getAuditMetadataString(metadata, 'new_owner_username');

  if (username) {
    return `target @${username}`;
  }

  const teamName =
    getAuditMetadataString(metadata, 'team_name') ||
    getAuditMetadataString(metadata, 'team_slug');
  if (teamName) {
    return `team ${teamName}`;
  }

  const packageName = getAuditMetadataString(metadata, 'package_name');
  const ecosystem = getAuditMetadataString(metadata, 'ecosystem');
  if (packageName && ecosystem) {
    return `package ${ecosystem} · ${packageName}`;
  }

  const namespace = getAuditMetadataString(metadata, 'namespace');
  if (namespace && ecosystem) {
    return `namespace ${ecosystem} · ${namespace}`;
  }

  if (namespace) {
    return `namespace ${namespace}`;
  }

  const orgName =
    getAuditMetadataString(metadata, 'org_name') ||
    getAuditMetadataString(metadata, 'org_slug') ||
    getAuditMetadataString(metadata, 'name') ||
    getAuditMetadataString(metadata, 'slug');
  if (orgName) {
    return `org ${orgName}`;
  }

  return null;
}

function formatAuditSummary(log: OrgAuditLog): string | null {
  const metadata = log.metadata;

  switch (log.action) {
    case 'org_create': {
      const name =
        getAuditMetadataString(metadata, 'name') ||
        getAuditMetadataString(metadata, 'slug');
      return name ? `Created ${name}.` : 'Created the organization workspace.';
    }
    case 'org_update':
      return formatOrgUpdateSummary(metadata);
    case 'namespace_claim_create': {
      const ecosystem = getAuditMetadataString(metadata, 'ecosystem');
      const namespace = getAuditMetadataString(metadata, 'namespace');
      if (ecosystem && namespace) {
        return `Created the ${ecosystemLabel(ecosystem)} namespace claim ${namespace}.`;
      }
      return namespace
        ? `Created the namespace claim ${namespace}.`
        : 'Created a namespace claim.';
    }
    case 'org_member_add': {
      const username = getAuditMetadataString(metadata, 'username');
      const role = getAuditMetadataString(metadata, 'role');
      if (username && role) {
        return `Granted ${formatRole(role)} to @${username}.`;
      }
      return username ? `Added @${username} to the organization.` : null;
    }
    case 'org_role_change': {
      const username = getAuditMetadataString(metadata, 'username');
      const previousRole = getAuditMetadataString(metadata, 'previous_role');
      const role = getAuditMetadataString(metadata, 'role');
      if (username && previousRole && role) {
        return `Changed @${username} from ${formatRole(previousRole)} to ${formatRole(role)}.`;
      }
      return username ? `Updated @${username}'s role.` : null;
    }
    case 'org_member_remove': {
      const username = getAuditMetadataString(metadata, 'username');
      const role = getAuditMetadataString(metadata, 'role');
      if (username && role) {
        return `Removed @${username} from the organization (${formatRole(role)}).`;
      }
      return username ? `Removed @${username} from the organization.` : null;
    }
    case 'org_ownership_transfer': {
      const newOwner = getAuditMetadataString(metadata, 'new_owner_username');
      const formerOwnerRole = getAuditMetadataString(
        metadata,
        'former_owner_new_role'
      );
      if (newOwner && formerOwnerRole) {
        return `Transferred ownership to @${newOwner}; the former owner is now ${formatRole(formerOwnerRole)}.`;
      }
      return newOwner
        ? `Transferred organization ownership to @${newOwner}.`
        : 'Transferred organization ownership.';
    }
    case 'org_invitation_create': {
      const invitedUsername = getAuditMetadataString(
        metadata,
        'invited_username'
      );
      const invitedEmail = getAuditMetadataString(metadata, 'invited_email');
      const role = getAuditMetadataString(metadata, 'role');
      const target = invitedUsername
        ? `@${invitedUsername}`
        : invitedEmail
          ? invitedEmail
          : 'the selected user';
      return role
        ? `Sent a ${formatRole(role)} invitation to ${target}.`
        : `Sent an invitation to ${target}.`;
    }
    case 'org_invitation_revoke': {
      const role = getAuditMetadataString(metadata, 'role');
      const username = log.target_username?.trim();
      if (role && username) {
        return `Revoked the ${formatRole(role)} invitation for @${username}.`;
      }
      return username
        ? `Revoked the invitation for @${username}.`
        : 'Revoked an active invitation.';
    }
    case 'org_invitation_accept': {
      const role = getAuditMetadataString(metadata, 'role');
      const username = log.target_username?.trim();
      if (role && username) {
        return `@${username} accepted a ${formatRole(role)} invitation.`;
      }
      return username ? `@${username} accepted an invitation.` : null;
    }
    case 'org_invitation_decline': {
      const role = getAuditMetadataString(metadata, 'role');
      const username = log.target_username?.trim();
      if (role && username) {
        return `@${username} declined a ${formatRole(role)} invitation.`;
      }
      return username ? `@${username} declined an invitation.` : null;
    }
    case 'team_create': {
      const teamName =
        getAuditMetadataString(metadata, 'team_name') ||
        getAuditMetadataString(metadata, 'team_slug');
      return teamName ? `Created team ${teamName}.` : 'Created a team.';
    }
    case 'team_update': {
      const teamName =
        getAuditMetadataString(metadata, 'team_name') ||
        getAuditMetadataString(metadata, 'team_slug');
      return teamName ? `Updated team ${teamName}.` : 'Updated a team.';
    }
    case 'team_delete': {
      const teamName =
        getAuditMetadataString(metadata, 'team_name') ||
        getAuditMetadataString(metadata, 'team_slug');
      return teamName ? `Deleted team ${teamName}.` : 'Deleted a team.';
    }
    case 'team_member_add': {
      const teamName =
        getAuditMetadataString(metadata, 'team_name') ||
        getAuditMetadataString(metadata, 'team_slug');
      const username = getAuditMetadataString(metadata, 'username');
      if (teamName && username) {
        return `Added @${username} to ${teamName}.`;
      }
      return username ? `Added @${username} to a team.` : null;
    }
    case 'team_member_remove': {
      const teamName =
        getAuditMetadataString(metadata, 'team_name') ||
        getAuditMetadataString(metadata, 'team_slug');
      const username = getAuditMetadataString(metadata, 'username');
      if (teamName && username) {
        return `Removed @${username} from ${teamName}.`;
      }
      return username ? `Removed @${username} from a team.` : null;
    }
    case 'team_package_access_update': {
      const teamName =
        getAuditMetadataString(metadata, 'team_name') ||
        getAuditMetadataString(metadata, 'team_slug') ||
        'the selected team';
      const packageName = getAuditMetadataString(metadata, 'package_name');
      const ecosystem = getAuditMetadataString(metadata, 'ecosystem');
      const permissions = getAuditMetadataStringArray(metadata, 'permissions');

      const packageLabel =
        packageName && ecosystem
          ? `${ecosystem} · ${packageName}`
          : packageName || 'the selected package';

      if (permissions.length === 0) {
        return `Removed delegated access to ${packageLabel} from ${teamName}.`;
      }

      return `Updated ${teamName} access to ${packageLabel}: ${permissions
        .map((permission) => formatPermission(permission))
        .join(', ')}.`;
    }
    default:
      return null;
  }
}

function getAuditMetadataString(
  metadata: Record<string, unknown> | null | undefined,
  key: string
): string | null {
  const value = metadata?.[key];
  return typeof value === 'string' && value.trim().length > 0
    ? value.trim()
    : null;
}

function getAuditMetadataStringArray(
  metadata: Record<string, unknown> | null | undefined,
  key: string
): string[] {
  const value = metadata?.[key];
  if (!Array.isArray(value)) {
    return [];
  }

  return value.filter((item): item is string => typeof item === 'string');
}

function getAuditMetadataRecord(
  metadata: Record<string, unknown> | null | undefined,
  key: string
): Record<string, unknown> | null {
  const value = metadata?.[key];
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }

  return value as Record<string, unknown>;
}

function formatOrgUpdateSummary(
  metadata: Record<string, unknown> | null | undefined
): string {
  const changes = getAuditMetadataRecord(metadata, 'changes');
  if (!changes) {
    return 'Updated the organization profile.';
  }

  const fieldUpdates = ['description', 'website', 'email']
    .map((field) => {
      const change = getAuditMetadataRecord(changes, field);
      if (!change) {
        return null;
      }

      const before = normalizeExistingOptionalText(
        getAuditMetadataString(change, 'before')
      );
      const after = normalizeExistingOptionalText(
        getAuditMetadataString(change, 'after')
      );
      const label = formatOrgProfileFieldLabel(field);

      if (before && after) {
        return `${label}: ${before} → ${after}`;
      }
      if (!before && after) {
        return `${label}: set to ${after}`;
      }
      if (before && !after) {
        return `${label}: cleared`;
      }

      return `${label}: updated`;
    })
    .filter((value): value is string => Boolean(value));

  return fieldUpdates.length > 0
    ? fieldUpdates.join('; ')
    : 'Updated the organization profile.';
}

function formatOrgProfileFieldLabel(field: string): string {
  switch (field) {
    case 'description':
      return 'Description';
    case 'website':
      return 'Website';
    case 'email':
      return 'Email';
    default:
      return formatIdentifierLabel(field);
  }
}

function renderPermissionBadges(permissions: string[]): string {
  if (permissions.length === 0) {
    return '';
  }

  return `
    <div class="token-row__scopes">
      ${permissions
        .map(
          (permission) =>
            `<span class="badge badge-ecosystem">${escapeHtml(formatPermission(permission))}</span>`
        )
        .join('')}
    </div>
  `;
}

function renderPackageSelectionOptions(packages: OrgPackageSummary[]): string {
  return [...packages]
    .sort((left, right) => {
      const leftKey = `${left.ecosystem || ''}:${left.name || ''}`;
      const rightKey = `${right.ecosystem || ''}:${right.name || ''}`;
      return leftKey.localeCompare(rightKey);
    })
    .map((pkg) => {
      const ecosystem = pkg.ecosystem || '';
      const name = pkg.name || '';
      const value = `${encodeURIComponent(ecosystem)}:${encodeURIComponent(name)}`;

      return `<option value="${value}">${escapeHtml(`${ecosystem} · ${name}`)}</option>`;
    })
    .join('');
}

function decodePackageSelection(
  value: string
): { ecosystem: string; name: string } | null {
  const separatorIndex = value.indexOf(':');
  if (separatorIndex <= 0 || separatorIndex === value.length - 1) {
    return null;
  }

  return {
    ecosystem: decodeURIComponent(value.slice(0, separatorIndex)),
    name: decodeURIComponent(value.slice(separatorIndex + 1)),
  };
}

function renderRoleOptions(selectedValue: string): string {
  return ORG_ROLE_OPTIONS.map(
    (role) => `
      <option value="${role.value}" ${role.value === selectedValue ? 'selected' : ''}>
        ${role.label}
      </option>
    `
  ).join('');
}

function formatRole(role: string): string {
  return role
    .split('_')
    .filter(Boolean)
    .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
    .join(' ');
}

function formatPermission(permission: string): string {
  return permission
    .split('_')
    .filter(Boolean)
    .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
    .join(' ');
}

function formatIdentifierLabel(value: string): string {
  return value
    .split('_')
    .filter(Boolean)
    .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
    .join(' ');
}

function getSubmitButton(form: HTMLFormElement): HTMLButtonElement | null {
  return form.querySelector<HTMLButtonElement>('button[type="submit"]');
}

function normalizeFormOptionalText(
  value: FormDataEntryValue | null | undefined
): string | null {
  if (typeof value !== 'string') {
    return null;
  }

  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function normalizeExistingOptionalText(
  value: string | null | undefined
): string | null {
  if (typeof value !== 'string') {
    return null;
  }

  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function hasTeamSlug(team: Team): team is Team & { slug: string } {
  return typeof team.slug === 'string' && team.slug.length > 0;
}

function toErrorMessage(error: unknown, fallback: string): string {
  return error instanceof Error && error.message ? error.message : fallback;
}
