import { ApiError, getAuthToken } from '../api/client';
import type {
  OrgAuditListResponse,
  OrgAuditLog,
  MemberListResponse,
  OrgInvitation,
  OrgInvitationListResponse,
  OrgMember,
  OrgPackageListResponse,
  OrgPackageSummary,
  OrganizationDetail,
  OrganizationListResponse,
  OrganizationMembership,
  Team,
  TeamListResponse,
  TeamMember,
  TeamMemberListResponse,
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
  listOrgAuditLogs,
  listMembers,
  listMyOrganizations,
  listOrgInvitations,
  listOrgPackages,
  listTeamPackageAccess,
  listTeamMembers,
  listTeams,
  removeTeamPackageAccess,
  removeMember,
  removeTeamMember,
  replaceTeamPackageAccess,
  revokeInvitation,
  sendInvitation,
  transferOwnership,
  updateTeam,
} from '../api/orgs';
import type { RouteContext } from '../router';
import { escapeHtml, formatDate, formatNumber } from '../utils/format';

const ADMIN_ROLES = new Set(['owner', 'admin']);
const ORG_AUDIT_PAGE_SIZE = 20;
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
  packages: OrgPackageSummary[];
  packagesError: string | null;
  auditLogs: OrgAuditLog[];
  auditError: string | null;
  invitations: OrgInvitation[];
  invitationsError: string | null;
  isAuthenticated: boolean;
  isOwner: boolean;
}

export function orgDetailPage(
  { params }: RouteContext,
  container: HTMLElement
): void {
  const slug = params.slug ?? '';

  container.innerHTML = `<div class="loading"><span class="spinner"></span> Loading organization…</div>`;
  void loadAndRender(container, slug);
}

async function loadAndRender(
  container: HTMLElement,
  slug: string,
  {
    notice = null,
    error = null,
  }: {
    notice?: string | null;
    error?: string | null;
  } = {}
): Promise<void> {
  try {
    const isAuthenticated = Boolean(getAuthToken());

    const [org, memberData, teamData, packageData, myOrganizationsData] =
      await Promise.all([
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

    const [
      invitationData,
      teamMembersBySlug,
      teamPackageAccessBySlug,
      auditData,
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
        ? listOrgAuditLogs(slug, { perPage: ORG_AUDIT_PAGE_SIZE }).catch(
            (caughtError: unknown): OrgAuditListResponse => ({
              logs: [],
              load_error: toErrorMessage(
                caughtError,
                'Failed to load the organization activity log.'
              ),
            })
          )
        : Promise.resolve<OrgAuditListResponse>({
            logs: [],
            load_error: null,
          }),
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
      packages: packageData.packages || [],
      packagesError: packageData.load_error || null,
      auditLogs: auditData.logs || [],
      auditError: auditData.load_error || null,
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
    packages,
    packagesError,
    auditLogs,
    auditError,
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
                  <p class="settings-copy">Showing the ${escapeHtml(String(ORG_AUDIT_PAGE_SIZE))} most recent governance events for this organization. This view is limited to owners and admins.</p>
                </div>
              </div>

              ${
                auditError
                  ? `<div class="alert alert-error">${escapeHtml(auditError)}</div>`
                  : auditLogs.length === 0
                    ? '<div class="empty-state"><h3>No activity yet</h3><p>Recent governance events will appear here once members, invitations, teams, and package access change.</p></div>'
                    : `<div class="token-list">${auditLogs.map((log) => renderAuditLogRow(log)).join('')}</div>`
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
            Use this page as the canonical workspace for organization ownership, members, teams, delegated package access, and visible packages.
            Audit and security dashboards continue to expand on this foundation.
          </p>
        </section>
      </div>

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
        </section>
      </div>
    </div>
  `;

  const inviteForm =
    container.querySelector<HTMLFormElement>('#org-invite-form');
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

function renderAuditLogRow(log: OrgAuditLog): string {
  const title = formatAuditActionLabel(log.action || 'activity');
  const actor = formatAuditActor(log);
  const target = formatAuditTarget(log);
  const summary = formatAuditSummary(log);
  const meta = [
    actor ? `by ${actor}` : null,
    target,
    log.occurred_at ? formatDate(log.occurred_at) : null,
  ].filter((value): value is string => Boolean(value));

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
    </div>
  `;
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

function formatAuditActionLabel(action: string): string {
  switch (action) {
    case 'org_create':
      return 'Organization created';
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

function hasTeamSlug(team: Team): team is Team & { slug: string } {
  return typeof team.slug === 'string' && team.slug.length > 0;
}

function toErrorMessage(error: unknown, fallback: string): string {
  return error instanceof Error && error.message ? error.message : fallback;
}
